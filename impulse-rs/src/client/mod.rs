//! Async client for daemon IPC.
//!
//! Wraps [`tokio::net::UnixStream`] with JSON-line protocol to communicate
//! with the Impulse daemon. Provides typed methods for session lifecycle,
//! file tracking, conflict detection, chat, and agent assist.

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::daemon::actor_provenance::{OperatorCapability, OPERATOR_CAPABILITY_ENV};
use crate::daemon::{DaemonRequest, DaemonResponse};

/// How long to wait for a daemon response before giving up. Generous enough for
/// slow LLM-backed handlers (the daemon's own LLM calls cap at ~120s) but
/// bounded so a hung/deadlocked daemon can't hang the CLI indefinitely.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(180);
const GOVERNED_VERIFICATION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(21 * 60);
const ACKNOWLEDGED_REQUEST_ATTEMPTS: usize = 2;

/// Write one JSON-line request and read exactly one JSON-line response.
///
/// Shared by the capability handshake and the request itself so both use the
/// same framing and the same hung-daemon timeout.
async fn exchange_line<W, R>(
    writer: &mut W,
    reader: &mut R,
    request: &DaemonRequest,
    timeout: Duration,
) -> Result<DaemonResponse>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: tokio::io::AsyncBufRead + Unpin,
{
    let request_json =
        serde_json::to_string(request).context("Failed to serialize daemon request")?;
    writer
        .write_all(request_json.as_bytes())
        .await
        .context("Failed to write request to daemon socket")?;
    writer
        .write_all(b"\n")
        .await
        .context("Failed to write newline delimiter to daemon socket")?;
    writer
        .flush()
        .await
        .context("Failed to flush daemon socket")?;

    let mut response_line = String::new();
    tokio::time::timeout(timeout, reader.read_line(&mut response_line))
        .await
        .with_context(|| {
            format!(
                "Daemon did not respond within {}s (it may be hung)",
                timeout.as_secs()
            )
        })?
        .context("Failed to read response from daemon socket")?;

    serde_json::from_str(&response_line).context("Failed to parse daemon response")
}

fn daemon_busy_error(
    resource: impulse_ops::DaemonBusyResource,
    retry_after_ms: u64,
) -> anyhow::Error {
    let resource = match resource {
        impulse_ops::DaemonBusyResource::AgentTurn => "agent turn",
    };
    anyhow!("Daemon {resource} is busy; retry after at least {retry_after_ms}ms")
}

pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    pub fn default_path() -> Self {
        Self::new(PathBuf::from(".impulse/sockets/impulse.sock"))
    }

    pub async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket_path)
            .await
            .context(format!(
                "Failed to connect to socket: {}",
                self.socket_path.display()
            ))
    }

    /// Where this client looks for the daemon run's operator capability
    /// (ADR-0018): beside the socket, as the daemon publishes it.
    pub fn operator_capability_path(&self) -> PathBuf {
        OperatorCapability::path_for_socket(&self.socket_path)
    }

    /// Resolve a capability token for this client, preferring an explicit
    /// environment override over the file the daemon publishes.
    ///
    /// Returns `None` when no capability is reachable — a launched governed
    /// pane, whose environment is scrubbed of every `IMPULSE_*` key, is exactly
    /// that case. The request is still sent, so the caller sees the daemon's
    /// typed unauthorized error rather than a client-side guess.
    fn operator_capability(&self) -> Option<OperatorCapability> {
        if let Ok(value) = std::env::var(OPERATOR_CAPABILITY_ENV) {
            if let Ok(capability) = OperatorCapability::parse(&value) {
                return Some(capability);
            }
        }
        let contents = std::fs::read_to_string(self.operator_capability_path()).ok()?;
        OperatorCapability::parse(&contents).ok()
    }

    /// Whether the daemon will refuse `request` from a non-operator connection.
    ///
    /// Deliberately coarse: every governed mutation presents the capability
    /// rather than the client re-deriving which mutations need it (that answer
    /// depends on the task's verification profile, which only the daemon
    /// holds). Other request groups skip the extra round trip entirely.
    fn requires_operator_class(request: &DaemonRequest) -> bool {
        matches!(request, DaemonRequest::MutateGovernedTask { .. })
    }

    pub async fn send(&self, request: DaemonRequest) -> Result<DaemonResponse> {
        self.send_with_timeout(request, RESPONSE_TIMEOUT).await
    }

    /// Send a request and await the response, failing if the daemon does not
    /// reply within `timeout` (so a hung daemon can't hang the caller forever).
    ///
    /// When the request needs operator class and a capability is reachable, it
    /// is presented first on the same connection: the daemon's classification
    /// is per connection, and this client opens one connection per request.
    async fn send_with_timeout(
        &self,
        request: DaemonRequest,
        timeout: Duration,
    ) -> Result<DaemonResponse> {
        let mut stream = self.connect().await?;
        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);

        if Self::requires_operator_class(&request) {
            if let Some(capability) = self.operator_capability() {
                let presentation = exchange_line(
                    &mut writer,
                    &mut reader,
                    &DaemonRequest::PresentOperatorCapability {
                        token: capability.expose().to_string(),
                    },
                    timeout,
                )
                .await?;
                if let DaemonResponse::Error { message } = &presentation {
                    // Not fatal here: the request below fails with the daemon's
                    // own typed authorization error, which explains more than a
                    // handshake failure would.
                    tracing::debug!(
                        reason = %message,
                        "operator capability presentation refused by daemon"
                    );
                }
            }
        }

        exchange_line(&mut writer, &mut reader, &request, timeout).await
    }

    pub async fn ping(&self) -> Result<bool> {
        let response = self.send(DaemonRequest::Ping).await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result
                .get("pong")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)),
            DaemonResponse::Error { message } => {
                anyhow::bail!("Ping failed: {}", message)
            }
            _ => anyhow::bail!("Ping: unexpected response type"),
        }
    }

    pub async fn status(&self) -> Result<serde_json::Value> {
        let response = self.send(DaemonRequest::Status).await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result),
            DaemonResponse::Error { message } => anyhow::bail!("Status failed: {}", message),
            _ => anyhow::bail!("Status: unexpected response type"),
        }
    }

    pub async fn create_session(
        &self,
        name: String,
        platform: Option<String>,
    ) -> Result<(String, String)> {
        let response = self
            .send(DaemonRequest::CreateSession { name, platform })
            .await?;

        match response {
            DaemonResponse::Ok { result } => {
                let session_id = result["session_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'session_id' in response"))?
                    .to_string();
                let session_name = result["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'name' in response"))?
                    .to_string();
                Ok((session_id, session_name))
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("Create session failed: {}", message)
            }
            _ => anyhow::bail!("Create session: unexpected response type"),
        }
    }

    pub async fn end_session(&self, session_id: String, summary: String) -> Result<String> {
        let response = self
            .send(DaemonRequest::EndSession {
                session_id,
                summary,
            })
            .await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result["session_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'session_id' in response"))?
                .to_string()),
            DaemonResponse::Error { message } => anyhow::bail!("End session failed: {}", message),
            _ => anyhow::bail!("End session: unexpected response type"),
        }
    }

    pub async fn track_file(&self, session_id: String, file_path: String) -> Result<()> {
        let response = self
            .send(DaemonRequest::TrackFile {
                session_id,
                file_path,
            })
            .await?;

        match response {
            DaemonResponse::Ok { .. } => Ok(()),
            DaemonResponse::Error { message } => anyhow::bail!("Track file failed: {}", message),
            _ => anyhow::bail!("Track file: unexpected response type"),
        }
    }

    pub async fn check_conflict(
        &self,
        session_id: String,
        file_path: String,
    ) -> Result<(bool, Vec<String>)> {
        let response = self
            .send(DaemonRequest::CheckConflict {
                session_id,
                file_path,
            })
            .await?;

        match response {
            DaemonResponse::ConflictCheck {
                has_conflict,
                conflicting_sessions,
            } => Ok((has_conflict, conflicting_sessions)),
            DaemonResponse::Error { message } => {
                anyhow::bail!("Check conflict failed: {}", message)
            }
            _ => anyhow::bail!("Check conflict: unexpected response type"),
        }
    }

    pub async fn track_tool(&self, session_id: String, tool_name: String) -> Result<()> {
        let response = self
            .send(DaemonRequest::TrackTool {
                session_id,
                tool_name,
            })
            .await?;

        match response {
            DaemonResponse::Ok { .. } => Ok(()),
            DaemonResponse::Error { message } => anyhow::bail!("Track tool failed: {}", message),
            _ => anyhow::bail!("Track tool: unexpected response type"),
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<serde_json::Value>> {
        let response = self.send(DaemonRequest::ListSessions).await?;

        match response {
            DaemonResponse::Ok { result } => {
                let sessions = result.as_array().cloned().unwrap_or_default();
                Ok(sessions)
            }
            DaemonResponse::Error { message } => anyhow::bail!("List sessions failed: {}", message),
            _ => anyhow::bail!("List sessions: unexpected response type"),
        }
    }

    pub async fn get_session(&self, session_id: String) -> Result<serde_json::Value> {
        let response = self.send(DaemonRequest::GetSession { session_id }).await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result),
            DaemonResponse::Error { message } => anyhow::bail!("Get session failed: {}", message),
            _ => anyhow::bail!("Get session: unexpected response type"),
        }
    }

    async fn governed_task_response(
        &self,
        request: DaemonRequest,
        operation: &str,
        timeout: Duration,
    ) -> Result<impulse_ops::governed_task::GovernedTaskRun> {
        let mut last_error = None;
        let mut response = None;
        for _ in 0..ACKNOWLEDGED_REQUEST_ATTEMPTS {
            match self.send_with_timeout(request.clone(), timeout).await {
                Ok(value) => {
                    response = Some(value);
                    break;
                }
                Err(error) => last_error = Some(error),
            }
        }
        let response = response.ok_or_else(|| {
            last_error.unwrap_or_else(|| anyhow!("{operation}: daemon request failed"))
        })?;
        match response {
            DaemonResponse::Ok { result } => serde_json::from_value(result)
                .with_context(|| format!("{operation}: invalid governed task response")),
            DaemonResponse::Error { message } => anyhow::bail!("{operation} failed: {message}"),
            DaemonResponse::Busy {
                resource,
                retry_after_ms,
            } => Err(daemon_busy_error(resource, retry_after_ms)),
            _ => anyhow::bail!("{operation}: unexpected response type"),
        }
    }

    pub async fn get_governed_task(
        &self,
        project_id: String,
        task_id: impulse_ops::governed_task::GovernedTaskId,
    ) -> Result<Option<impulse_ops::governed_task::GovernedTaskRun>> {
        match self
            .send(DaemonRequest::GetGovernedTask {
                project_id,
                task_id,
            })
            .await?
        {
            DaemonResponse::Ok { result } => {
                serde_json::from_value(result).context("get governed task: invalid daemon response")
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("get governed task failed: {message}")
            }
            DaemonResponse::Busy {
                resource,
                retry_after_ms,
            } => Err(daemon_busy_error(resource, retry_after_ms)),
            _ => anyhow::bail!("get governed task: unexpected response type"),
        }
    }

    pub async fn submit_governed_claim(
        &self,
        request: impulse_ops::governed_task::GovernedClaimRequest,
    ) -> Result<impulse_ops::governed_task::GovernedTaskRun> {
        self.governed_task_response(
            DaemonRequest::SubmitGovernedClaim { request },
            "submit governed claim",
            RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn run_governed_verification(
        &self,
        request: impulse_ops::governed_task::GovernedVerificationRequest,
    ) -> Result<impulse_ops::governed_task::GovernedTaskRun> {
        self.governed_task_response(
            DaemonRequest::RunGovernedVerification { request },
            "run governed verification",
            GOVERNED_VERIFICATION_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn run_governed_supervisor_review(
        &self,
        request: impulse_ops::governed_task::GovernedSupervisorReviewRequest,
    ) -> Result<impulse_ops::governed_task::GovernedTaskRun> {
        self.governed_task_response(
            DaemonRequest::RunGovernedSupervisorReview { request },
            "run governed Supervisor review",
            RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn chat(
        &self,
        session_id: String,
        message: String,
        inject_mode: Option<String>,
        inject_explain: bool,
    ) -> Result<serde_json::Value> {
        let response = self
            .send(DaemonRequest::Chat {
                session_id,
                message,
                inject_mode,
                inject_explain,
            })
            .await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result),
            DaemonResponse::Error { message } => anyhow::bail!("Chat failed: {}", message),
            _ => anyhow::bail!("Chat: unexpected response type"),
        }
    }

    pub async fn agent_assist(&self, prompt: &str, context: Option<&str>) -> Result<String> {
        self.agent_assist_with_insights(prompt, context, Vec::new())
            .await
    }

    /// Agent assist with extracted insights for cross-pane context enrichment.
    pub async fn agent_assist_with_insights(
        &self,
        prompt: &str,
        context: Option<&str>,
        insights: Vec<crate::context_lifecycle::types::ExtractedInsight>,
    ) -> Result<String> {
        let response = self
            .send(DaemonRequest::AgentAssist {
                prompt: prompt.to_string(),
                context: context.map(|c| c.to_string()),
                insights,
            })
            .await?;

        match response {
            DaemonResponse::AgentAssistResult {
                success, response, ..
            } => {
                if success {
                    Ok(response)
                } else {
                    anyhow::bail!("Agent assist failed: {}", response)
                }
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("Agent assist failed: {}", message)
            }
            DaemonResponse::Busy {
                resource,
                retry_after_ms,
            } => Err(daemon_busy_error(resource, retry_after_ms)),
            _ => anyhow::bail!("Agent assist: unexpected response type"),
        }
    }

    /// Get the conflict resolution history from the daemon's ConflictResolver.
    pub async fn get_conflict_history(
        &self,
    ) -> Result<Vec<crate::agent::coordinator::ConflictRecord>> {
        let response = self.send(DaemonRequest::GetConflictHistory).await?;

        match response {
            DaemonResponse::Ok { result } => {
                let history: Vec<crate::agent::coordinator::ConflictRecord> =
                    serde_json::from_value(result)
                        .context("Failed to parse conflict history response")?;
                Ok(history)
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("Get conflict history failed: {}", message)
            }
            _ => anyhow::bail!("Get conflict history: unexpected response type"),
        }
    }

    /// Request a code review via the Impulse Agent (specialized endpoint).
    pub async fn agent_review_code(
        &self,
        file_path: &str,
        diff: &str,
        insights: Vec<crate::context_lifecycle::types::ExtractedInsight>,
    ) -> Result<String> {
        let response = self
            .send(DaemonRequest::AgentReviewCode {
                file_path: file_path.to_string(),
                diff: diff.to_string(),
                insights,
            })
            .await?;

        match response {
            DaemonResponse::AgentSpecializedResult {
                success, response, ..
            } => {
                if success {
                    Ok(response)
                } else {
                    anyhow::bail!("Agent review_code failed: {}", response)
                }
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("Agent review_code failed: {}", message)
            }
            DaemonResponse::Busy {
                resource,
                retry_after_ms,
            } => Err(daemon_busy_error(resource, retry_after_ms)),
            _ => anyhow::bail!("Agent review_code: unexpected response type"),
        }
    }

    /// Request error analysis via the Impulse Agent (specialized endpoint).
    pub async fn agent_analyze_error(
        &self,
        error_text: &str,
        context: &str,
        insights: Vec<crate::context_lifecycle::types::ExtractedInsight>,
    ) -> Result<String> {
        let response = self
            .send(DaemonRequest::AgentAnalyzeError {
                error_text: error_text.to_string(),
                context: context.to_string(),
                insights,
            })
            .await?;

        match response {
            DaemonResponse::AgentSpecializedResult {
                success, response, ..
            } => {
                if success {
                    Ok(response)
                } else {
                    anyhow::bail!("Agent analyze_error failed: {}", response)
                }
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("Agent analyze_error failed: {}", message)
            }
            DaemonResponse::Busy {
                resource,
                retry_after_ms,
            } => Err(daemon_busy_error(resource, retry_after_ms)),
            _ => anyhow::bail!("Agent analyze_error: unexpected response type"),
        }
    }

    /// Request a pane summary via the Impulse Agent (specialized endpoint).
    pub async fn agent_summarize_pane(
        &self,
        pane_id: usize,
        raw_output: &str,
        insights: Vec<crate::context_lifecycle::types::ExtractedInsight>,
    ) -> Result<String> {
        let response = self
            .send(DaemonRequest::AgentSummarizePane {
                pane_id,
                raw_output: raw_output.to_string(),
                insights,
            })
            .await?;

        match response {
            DaemonResponse::AgentSpecializedResult {
                success, response, ..
            } => {
                if success {
                    Ok(response)
                } else {
                    anyhow::bail!("Agent summarize_pane failed: {}", response)
                }
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("Agent summarize_pane failed: {}", message)
            }
            DaemonResponse::Busy {
                resource,
                retry_after_ms,
            } => Err(daemon_busy_error(resource, retry_after_ms)),
            _ => anyhow::bail!("Agent summarize_pane: unexpected response type"),
        }
    }

    /// Clear resolved conflicts from the daemon's ConflictResolver.
    pub async fn clear_resolved_conflicts(&self) -> Result<bool> {
        let response = self.send(DaemonRequest::ClearResolvedConflicts).await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result
                .get("cleared")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)),
            DaemonResponse::Error { message } => {
                anyhow::bail!("Clear resolved conflicts failed: {}", message)
            }
            _ => anyhow::bail!("Clear resolved conflicts: unexpected response type"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_client_new_stores_path() {
        let client = DaemonClient::new(PathBuf::from("/tmp/test.sock"));
        assert_eq!(client.socket_path, PathBuf::from("/tmp/test.sock"));
    }

    fn approval_request() -> DaemonRequest {
        use impulse_ops::governed_task::{
            GovernedActor, GovernedActorKind, GovernedRecordId, GovernedRequestId, GovernedTaskId,
            GovernedTaskMutation, GovernedTaskMutationRequest, OperatorDecisionInput,
            OperatorDecisionKind,
        };
        DaemonRequest::MutateGovernedTask {
            request: GovernedTaskMutationRequest {
                request_id: GovernedRequestId::try_new("approve-1").unwrap(),
                project_id: "project-a".to_string(),
                task_id: GovernedTaskId::try_new("task-a").unwrap(),
                expected_revision: 4,
                mutation: GovernedTaskMutation::RecordOperatorDecision {
                    decision: OperatorDecisionInput {
                        actor: GovernedActor {
                            kind: GovernedActorKind::Operator,
                            id: "operator-a".to_string(),
                        },
                        supervisor_verdict_id: GovernedRecordId::try_new("verdict-a").unwrap(),
                        decision: OperatorDecisionKind::Approve,
                        rationale: "approved".to_string(),
                    },
                },
            },
        }
    }

    #[test]
    fn test_only_governed_mutations_trigger_a_capability_handshake() {
        assert!(DaemonClient::requires_operator_class(&approval_request()));
        for request in [
            DaemonRequest::Ping,
            DaemonRequest::Status,
            DaemonRequest::ListGovernedTasks {
                project_id: "project-a".to_string(),
            },
        ] {
            assert!(
                !DaemonClient::requires_operator_class(&request),
                "reads must not pay for a handshake"
            );
        }
    }

    #[test]
    fn test_operator_capability_path_sits_beside_the_socket() {
        let client = DaemonClient::new(PathBuf::from("/tmp/sockets/impulse.sock"));
        assert_eq!(
            client.operator_capability_path(),
            PathBuf::from("/tmp/sockets/impulse.operator-cap")
        );
    }

    #[test]
    fn test_operator_capability_reads_the_published_file_and_rejects_junk() {
        let dir = tempfile::tempdir().unwrap();
        let client = DaemonClient::new(dir.path().join("impulse.sock"));
        assert!(
            client.operator_capability().is_none(),
            "an unpublished capability must resolve to None, not a panic"
        );

        let capability = OperatorCapability::generate();
        std::fs::write(
            client.operator_capability_path(),
            format!("{}\n", capability.expose()),
        )
        .unwrap();
        assert_eq!(client.operator_capability().unwrap(), capability);

        std::fs::write(client.operator_capability_path(), "not-a-capability").unwrap();
        assert!(client.operator_capability().is_none());
    }

    #[tokio::test]
    async fn test_governed_mutation_presents_the_capability_before_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("impulse.sock");
        let capability = OperatorCapability::generate();
        std::fs::write(
            OperatorCapability::path_for_socket(&socket),
            capability.expose(),
        )
        .unwrap();
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();

        let expected_token = capability.expose().to_string();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader);
            let mut received = Vec::new();
            for response in [
                serde_json::json!({"type": "Ok", "data": {"result": {"connection_class": "operator"}}}),
                serde_json::json!({"type": "Ok", "data": {"result": {"accepted": true}}}),
            ] {
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                received.push(serde_json::from_str::<DaemonRequest>(&line).unwrap());
                writer
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .unwrap();
                writer.flush().await.unwrap();
            }
            received
        });

        let client = DaemonClient::new(socket);
        client.send(approval_request()).await.unwrap();
        let received = server.await.unwrap();

        assert!(
            matches!(
                &received[0],
                DaemonRequest::PresentOperatorCapability { token } if token == &expected_token
            ),
            "the capability must be presented first, on the same connection"
        );
        assert!(matches!(
            received[1],
            DaemonRequest::MutateGovernedTask { .. }
        ));
    }

    #[tokio::test]
    async fn test_governed_mutation_without_a_capability_sends_only_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("impulse.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer
                .write_all(
                    b"{\"type\": \"Error\", \"data\": {\"message\": \"operator capability required\"}}\n",
                )
                .await
                .unwrap();
            writer.flush().await.unwrap();
            serde_json::from_str::<DaemonRequest>(&line).unwrap()
        });

        let client = DaemonClient::new(socket);
        let response = client.send(approval_request()).await.unwrap();
        assert!(matches!(response, DaemonResponse::Error { .. }));
        assert!(
            matches!(
                server.await.unwrap(),
                DaemonRequest::MutateGovernedTask { .. }
            ),
            "a scrubbed caller sends the request and surfaces the daemon's typed refusal"
        );
    }

    #[test]
    fn test_daemon_busy_error_preserves_resource_and_retry_hint() {
        let error = daemon_busy_error(impulse_ops::DaemonBusyResource::AgentTurn, 250);
        let message = error.to_string();
        assert!(message.contains("agent turn"));
        assert!(message.contains("250ms"));
    }

    #[tokio::test]
    async fn test_send_times_out_when_daemon_never_responds() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("hang.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();

        // Accept the connection but never send a response.
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let _hold = stream;
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });

        let client = DaemonClient::new(sock);
        let start = std::time::Instant::now();
        let result = client
            .send_with_timeout(DaemonRequest::Ping, Duration::from_millis(200))
            .await;

        assert!(
            result.is_err(),
            "an unresponsive daemon must yield an error"
        );
        assert!(
            result.unwrap_err().to_string().contains("did not respond"),
            "error should explain the timeout"
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the call must fail promptly, not hang"
        );
    }

    #[test]
    fn test_daemon_client_default_path() {
        let client = DaemonClient::default_path();
        assert!(client
            .socket_path
            .to_str()
            .unwrap()
            .contains("impulse.sock"));
    }

    #[test]
    fn test_daemon_request_serialization() {
        let req = DaemonRequest::Ping;
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.is_empty());

        let req = DaemonRequest::CreateSession {
            name: "test".into(),
            platform: Some("claude".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("test"));
    }

    #[test]
    fn test_get_conflict_history_request_serialization() {
        let req = DaemonRequest::GetConflictHistory;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("GetConflictHistory"));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::GetConflictHistory));
    }

    #[test]
    fn test_clear_resolved_conflicts_request_serialization() {
        let req = DaemonRequest::ClearResolvedConflicts;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("ClearResolvedConflicts"));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::ClearResolvedConflicts));
    }

    #[test]
    fn test_agent_review_code_request_serialization() {
        let req = DaemonRequest::AgentReviewCode {
            file_path: "src/main.rs".to_string(),
            diff: "+ added line".to_string(),
            insights: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("AgentReviewCode"));
        assert!(json.contains("src/main.rs"));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::AgentReviewCode { .. }));
    }

    #[test]
    fn test_agent_analyze_error_request_serialization() {
        let req = DaemonRequest::AgentAnalyzeError {
            error_text: "error[E0502]: cannot borrow".to_string(),
            context: "pane-1".to_string(),
            insights: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("AgentAnalyzeError"));
        assert!(json.contains("E0502"));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::AgentAnalyzeError { .. }));
    }

    #[test]
    fn test_agent_summarize_pane_request_serialization() {
        let req = DaemonRequest::AgentSummarizePane {
            pane_id: 3,
            raw_output: "some terminal output".to_string(),
            insights: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("AgentSummarizePane"));
        assert!(json.contains("\"pane_id\":3"));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::AgentSummarizePane { .. }));
    }

    #[test]
    fn test_agent_review_code_insights_default_empty() {
        // Ensure #[serde(default)] works — omit insights from JSON
        let json = r#"{"type":"AgentReviewCode","data":{"file_path":"test.rs","diff":""}}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::AgentReviewCode { insights, .. } => {
                assert!(insights.is_empty(), "insights should default to empty vec");
            }
            _ => panic!("Expected AgentReviewCode"),
        }
    }

    #[test]
    fn test_agent_analyze_error_insights_default_empty() {
        let json = r#"{"type":"AgentAnalyzeError","data":{"error_text":"err","context":"ctx"}}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::AgentAnalyzeError { insights, .. } => {
                assert!(insights.is_empty(), "insights should default to empty vec");
            }
            _ => panic!("Expected AgentAnalyzeError"),
        }
    }

    #[test]
    fn test_agent_summarize_pane_insights_default_empty() {
        let json = r#"{"type":"AgentSummarizePane","data":{"pane_id":1,"raw_output":"output"}}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::AgentSummarizePane {
                insights, pane_id, ..
            } => {
                assert_eq!(pane_id, 1);
                assert!(insights.is_empty(), "insights should default to empty vec");
            }
            _ => panic!("Expected AgentSummarizePane"),
        }
    }

    #[test]
    fn test_agent_specialized_result_serialization() {
        let resp = DaemonResponse::AgentSpecializedResult {
            success: true,
            response: "review complete".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("AgentSpecializedResult"));
        assert!(json.contains("review complete"));
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonResponse::AgentSpecializedResult { success, response } => {
                assert!(success);
                assert_eq!(response, "review complete");
            }
            _ => panic!("Expected AgentSpecializedResult"),
        }
    }
}
