use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::daemon::{read_bounded_line, BoundedLine, MAX_REQUEST_SIZE};
use crate::tooling::{ToolContext, ToolRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransport {
    Stdio,
    Tcp(u16),
}

pub struct McpServer {
    registry: Arc<ToolRegistry>,
    ctx: ToolContext,
}

impl McpServer {
    pub fn new(registry: Arc<ToolRegistry>, ctx: ToolContext) -> Self {
        Self { registry, ctx }
    }

    pub async fn serve(&self, transport: McpTransport) -> Result<()> {
        match transport {
            McpTransport::Stdio => self.serve_stdio().await,
            McpTransport::Tcp(port) => self.serve_tcp(port).await,
        }
    }

    /// **Bounded reads (same-day fix; see `daemon::mod::read_bounded_line`'s
    /// doc for the full rationale):** this loop previously called
    /// `reader.read_line(&mut line)` with no size cap at all -- unlike
    /// `daemon::mod`'s connection loop, which at least had a (too-late)
    /// post-hoc check, this stdio path had no guard whatsoever. Any process
    /// piping stdin into this server could send unbounded non-newline bytes
    /// and OOM it. Reuses the daemon's `read_bounded_line`/`MAX_REQUEST_SIZE`
    /// so both JSON-line protocol readers in this codebase share one bound
    /// and one implementation.
    async fn serve_stdio(&self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut writer = tokio::io::BufWriter::new(stdout);

        loop {
            let line = match read_bounded_line(&mut reader, MAX_REQUEST_SIZE).await? {
                BoundedLine::Eof => break,
                BoundedLine::TooLarge => {
                    let response = serde_json::json!({
                        "error": {
                            "code": -32600,
                            "message": format!("Request too large (max {} bytes)", MAX_REQUEST_SIZE)
                        }
                    });
                    writer
                        .write_all(serde_json::to_string(&response)?.as_bytes())
                        .await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                    // Same reasoning as `daemon::mod::handle_connection`: the
                    // remaining bytes of this oversized "line" are of
                    // unknown length, so there is no bounded way to
                    // resynchronize on the next `\n`. Close the stream.
                    break;
                }
                BoundedLine::Line(line) => line,
            };
            let response = self.process_request(&line).await;
            writer
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }

        Ok(())
    }

    /// **Bounded reads (same-day fix):** `serve_tcp` binds to
    /// `127.0.0.1`, which is reachable by *any* local process on the
    /// machine, not just this daemon's own clients -- unlike stdio, this is
    /// a genuine local-multi-tenant attack surface. It had the same
    /// unbounded `read_line` with no size guard at all as `serve_stdio`;
    /// see [`Self::serve_stdio`]'s doc and `daemon::mod::read_bounded_line`
    /// for the full rationale. Reuses the same helper/constant.
    async fn serve_tcp(&self, port: u16) -> Result<()> {
        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        println!("MCP compatibility server listening on {}", addr);

        loop {
            // A failed accept (EMFILE, a connection reset before accept)
            // used to end the server; it now waits briefly and goes on, as
            // the voice transports do.
            let socket = match listener.accept().await {
                Ok((socket, _)) => socket,
                Err(error) => {
                    tracing::warn!("MCP TCP accept failed: {error}");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            };
            let registry = self.registry.clone();
            let ctx = self.ctx.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = socket.into_split();
                let mut reader = BufReader::new(reader);
                let server = McpServer::new(registry, ctx);

                loop {
                    let line = match read_bounded_line(&mut reader, MAX_REQUEST_SIZE).await {
                        Ok(BoundedLine::Eof) => break,
                        Ok(BoundedLine::TooLarge) => {
                            let response = serde_json::json!({
                                "error": {
                                    "code": -32600,
                                    "message": format!("Request too large (max {} bytes)", MAX_REQUEST_SIZE)
                                }
                            });
                            let _ = writer
                                .write_all(
                                    serde_json::to_string(&response)
                                        .unwrap_or_default()
                                        .as_bytes(),
                                )
                                .await;
                            let _ = writer.write_all(b"\n").await;
                            break;
                        }
                        Ok(BoundedLine::Line(line)) => line,
                        Err(_) => break,
                    };

                    let response = server.process_request(&line).await;
                    if writer
                        .write_all(
                            serde_json::to_string(&response)
                                .unwrap_or_else(|_| {
                                    serde_json::json!({
                                        "error": {
                                            "code": -32603,
                                            "message": "failed to serialize response"
                                        }
                                    })
                                    .to_string()
                                })
                                .as_bytes(),
                        )
                        .await
                        .is_err()
                    {
                        break;
                    }
                    if writer.write_all(b"\n").await.is_err() {
                        break;
                    }
                }
            });
        }
    }

    pub async fn process_request(&self, request_str: &str) -> serde_json::Value {
        let request: serde_json::Value = match serde_json::from_str(request_str) {
            Ok(value) => value,
            Err(err) => {
                return serde_json::json!({
                    "error": {"code": -32700, "message": format!("Parse error: {}", err)}
                });
            }
        };

        let method = request
            .get("method")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        match method {
            "tools/list" => {
                let tools: Vec<_> = self
                    .registry
                    .schema_json()
                    .into_iter()
                    .map(|tool| {
                        serde_json::json!({
                            "name": tool["name"],
                            "description": tool["description"],
                            "input_schema": tool["input_schema"],
                            "capabilities": tool["capabilities"],
                            "source": tool["source"],
                        })
                    })
                    .collect();
                serde_json::json!({ "tools": tools })
            }
            "tools/call" => {
                let params = request.get("params").unwrap_or(&request);
                let name = params
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                match self.registry.execute(name, arguments, &self.ctx).await {
                    Ok(result) => render_tool_result(result.output),
                    Err(err) => serde_json::json!({
                        "content": [{"type": "text", "text": format!("Error: {}", err)}],
                        "isError": true
                    }),
                }
            }
            "resources/list" => self.list_resources(),
            "resources/read" => {
                let params = request.get("params").unwrap_or(&request);
                let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                self.read_resource(uri)
            }
            _ => serde_json::json!({
                "error": {"code": -32601, "message": "Method not found"}
            }),
        }
    }
}

impl McpServer {
    /// List available Impulse resources for MCP clients.
    fn list_resources(&self) -> serde_json::Value {
        let base = &self.ctx.impulse_dir;
        let mut resources = Vec::new();

        // Core data files that Impulse manages.
        let resource_defs = [
            (
                "impulse://genome",
                "GENOME.md",
                "Permanent decisions and preferences",
                "text/markdown",
            ),
            (
                "impulse://history",
                "HISTORY.jsonl",
                "Append-only session log",
                "application/jsonl",
            ),
            (
                "impulse://live-state",
                "LIVE_STATE.json",
                "Active session state (ephemeral)",
                "application/json",
            ),
            (
                "impulse://config",
                "config.json",
                "Runtime configuration",
                "application/json",
            ),
        ];

        for (uri, filename, desc, mime) in &resource_defs {
            let path = base.join(filename);
            if path.exists() {
                resources.push(serde_json::json!({
                    "uri": uri,
                    "name": filename,
                    "description": desc,
                    "mimeType": mime,
                }));
            }
        }

        serde_json::json!({ "resources": resources })
    }

    /// Read an Impulse resource by URI.
    fn read_resource(&self, uri: &str) -> serde_json::Value {
        let base = &self.ctx.impulse_dir;

        let filename = match uri {
            "impulse://genome" => "GENOME.md",
            "impulse://history" => "HISTORY.jsonl",
            "impulse://live-state" => "LIVE_STATE.json",
            "impulse://config" => "config.json",
            _ => {
                return serde_json::json!({
                    "error": {"code": -32602, "message": format!("Unknown resource URI: {}", uri)}
                });
            }
        };

        let path = base.join(filename);
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let (content, mime) = resource_body(filename, content);
                serde_json::json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": mime,
                        "text": content,
                    }]
                })
            }
            Err(e) => serde_json::json!({
                "error": {"code": -32603, "message": format!("Failed to read {}: {}", filename, e)}
            }),
        }
    }
}

/// The text and MIME type a resource file is served as. `add-decision` and
/// `init` store GENOME.md as JSON, which was served labelled
/// `text/markdown`; it is served as the Markdown it renders to now. A
/// hand-written Markdown genome is served as it is, and JSON that isn't a
/// genome is labelled as JSON.
fn resource_body(filename: &str, content: String) -> (String, &'static str) {
    if filename.ends_with(".md") {
        if let Ok(genome) = serde_json::from_str::<crate::memory::Genome>(&content) {
            return (genome.to_markdown(), "text/markdown");
        }
        if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
            return (content, "application/json");
        }
        return (content, "text/markdown");
    }
    let mime = if filename.ends_with(".jsonl") {
        "application/jsonl"
    } else {
        "application/json"
    };
    (content, mime)
}

fn render_tool_result(output: serde_json::Value) -> serde_json::Value {
    match output {
        serde_json::Value::String(text) => {
            serde_json::json!({ "content": [{"type": "text", "text": text}] })
        }
        other => serde_json::json!({ "content": [{"type": "json", "json": other}] }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tooling::{
        Capability, DynamicTool, ExecutionOrigin, ToolCategory, ToolContext, ToolDescriptor,
        ToolError, ToolRegistry, ToolResult,
    };
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct AdmissionProbe {
        id: &'static str,
        capability: Capability,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl DynamicTool for AdmissionProbe {
        fn id(&self) -> &str {
            self.id
        }

        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                id: self.id.into(),
                name: self.id.into(),
                description: "Admission test probe".into(),
                version: "1.0.0".into(),
                category: ToolCategory::Utility,
                params: Vec::new(),
            }
        }

        fn validate_params(
            &self,
            params: &serde_json::Value,
        ) -> std::result::Result<(), ToolError> {
            if params.is_object() {
                Ok(())
            } else {
                Err(ToolError::InvalidParams(
                    "arguments must be an object".into(),
                ))
            }
        }

        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &ToolContext,
        ) -> std::result::Result<ToolResult, ToolError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ToolResult::text("probe executed"))
        }

        fn required_capabilities(&self) -> Vec<Capability> {
            vec![self.capability]
        }
    }

    fn test_server() -> McpServer {
        McpServer::new(
            Arc::new(ToolRegistry::with_defaults()),
            ToolContext {
                execution_origin: ExecutionOrigin::Test,
                ..ToolContext::with_all_capabilities()
            },
        )
    }

    async fn invoke_daemon_request(
        request: crate::daemon::protocol::DaemonRequest,
        registry: &ToolRegistry,
        tool_context: &ToolContext,
        state: &crate::state::SharedState,
    ) -> crate::daemon::protocol::DaemonResponse {
        use crate::daemon::actor_provenance::ConnectionClass;
        use crate::daemon::handlers::ProcessRequestContext;

        let wire = serde_json::to_vec(&request).unwrap();
        let request = serde_json::from_slice(&wire).unwrap();
        let terminal_telemetry = Arc::new(tokio::sync::RwLock::new(
            crate::ops_workbench::TerminalOpsTelemetryStore::default(),
        ));
        let supervisor_session_override = Arc::new(tokio::sync::RwLock::new(None));
        let conflict_resolver = Arc::new(tokio::sync::RwLock::new(
            crate::agent::coordinator::ConflictResolver::new(),
        ));
        let delegation_tracker = Arc::new(tokio::sync::RwLock::new(
            crate::delegation::DelegationTracker::new(),
        ));
        let cached_agent: Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>> =
            Arc::new(tokio::sync::Mutex::new(None));

        crate::daemon::handlers::process_request(
            request,
            ProcessRequestContext {
                state: state.clone(),
                registry,
                tool_context,
                terminal_telemetry: &terminal_telemetry,
                supervisor_session_override: &supervisor_session_override,
                conflict_resolver: &conflict_resolver,
                delegation_tracker: &delegation_tracker,
                cached_agent: &cached_agent,
                connection_class: ConnectionClass::NonOperator,
            },
        )
        .await
    }

    #[tokio::test]
    async fn test_mcp_request_parsing() {
        let response = test_server()
            .process_request(r#"{"method":"tools/list"}"#)
            .await;
        assert!(response["tools"].is_array());
        assert!(response["tools"][0]["source"].is_string());
    }

    #[tokio::test]
    async fn test_mcp_tools_list_does_not_advertise_document_extract() {
        // Stage 1b-B: document_extract was a stub whose default path always
        // errored ("Python extraction not yet wired") and is superseded by
        // document_read (xlsx/csv/docx/pdf/txt/md). It was deleted from
        // src/tooling/builtin, so MCP's tools/list -- which enumerates
        // whatever ToolRegistry has registered -- must no longer list it.
        let response = test_server()
            .process_request(r#"{"method":"tools/list"}"#)
            .await;
        let tools = response["tools"].as_array().expect("tools is an array");
        assert!(
            !tools.iter().any(|tool| tool["name"] == "document_extract"),
            "document_extract must not be advertised: {tools:?}"
        );
    }

    #[tokio::test]
    async fn test_mcp_tool_response() {
        let response = test_server()
            .process_request(
                r#"{"method":"tools/call","params":{"name":"system_info","arguments":{}}}"#,
            )
            .await;
        assert!(response["content"].is_array());
        assert_ne!(response["isError"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn test_mcp_and_daemon_tool_calls_share_deny_by_default_admission() {
        let read_calls = Arc::new(AtomicUsize::new(0));
        let write_calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(AdmissionProbe {
                id: "read_probe",
                capability: Capability::FileSystemRead,
                calls: read_calls.clone(),
            }))
            .unwrap();
        registry
            .register(Box::new(AdmissionProbe {
                id: "write_probe",
                capability: Capability::FileSystemWrite,
                calls: write_calls.clone(),
            }))
            .unwrap();
        let registry = Arc::new(registry);
        let temp = tempfile::tempdir().unwrap();
        let state = Arc::new(crate::state::State::new(temp.path().to_path_buf()).unwrap());
        let config = crate::state::Config::default();
        let mcp_context = crate::handlers::build_remote_tool_context(
            temp.path(),
            &config,
            ExecutionOrigin::Mcp,
            None,
        );
        let daemon_context = crate::handlers::build_remote_tool_context(
            temp.path(),
            &config,
            ExecutionOrigin::Daemon,
            None,
        );
        assert_eq!(
            mcp_context.allowed_capabilities,
            daemon_context.allowed_capabilities
        );
        assert!(mcp_context.has_capability(Capability::FileSystemRead));
        assert!(!mcp_context.has_capability(Capability::FileSystemWrite));

        let mcp = McpServer::new(registry.clone(), mcp_context);
        let mcp_read = mcp
            .process_request(
                r#"{"method":"tools/call","params":{"name":"read_probe","arguments":{}}}"#,
            )
            .await;
        assert_ne!(mcp_read["isError"], serde_json::json!(true));
        assert_eq!(read_calls.load(Ordering::SeqCst), 1);

        let mcp_bad_args = mcp
            .process_request(
                r#"{"method":"tools/call","params":{"name":"read_probe","arguments":[]}}"#,
            )
            .await;
        assert_eq!(mcp_bad_args["isError"], serde_json::json!(true));
        assert!(mcp_bad_args["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Invalid parameters"));
        assert_eq!(read_calls.load(Ordering::SeqCst), 1);

        let mcp_denied = mcp
            .process_request(
                r#"{"method":"tools/call","params":{"name":"write_probe","arguments":{}}}"#,
            )
            .await;
        assert_eq!(mcp_denied["isError"], serde_json::json!(true));
        assert!(mcp_denied["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Missing required capability"));
        assert_eq!(write_calls.load(Ordering::SeqCst), 0);

        let daemon_denied = invoke_daemon_request(
            crate::daemon::protocol::DaemonRequest::InvokeTool {
                name: "write_probe".into(),
                params: serde_json::json!({}),
            },
            registry.as_ref(),
            &daemon_context,
            &state,
        )
        .await;
        match daemon_denied {
            crate::daemon::protocol::DaemonResponse::Error { message } => {
                assert!(message.contains("Missing required capability"), "{message}");
            }
            other => panic!("expected a denied tool error, got {other:?}"),
        }
        assert_eq!(write_calls.load(Ordering::SeqCst), 0);

        let daemon_bad_args = invoke_daemon_request(
            crate::daemon::protocol::DaemonRequest::InvokeTool {
                name: "read_probe".into(),
                params: serde_json::json!([]),
            },
            registry.as_ref(),
            &daemon_context,
            &state,
        )
        .await;
        match daemon_bad_args {
            crate::daemon::protocol::DaemonResponse::Error { message } => {
                assert!(message.contains("Invalid parameters"), "{message}");
            }
            other => panic!("expected malformed arguments to fail, got {other:?}"),
        }
        assert_eq!(read_calls.load(Ordering::SeqCst), 1);

        let daemon_read = invoke_daemon_request(
            crate::daemon::protocol::DaemonRequest::InvokeTool {
                name: "read_probe".into(),
                params: serde_json::json!({}),
            },
            registry.as_ref(),
            &daemon_context,
            &state,
        )
        .await;
        assert!(matches!(
            daemon_read,
            crate::daemon::protocol::DaemonResponse::Ok { .. }
        ));
        assert_eq!(read_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_mcp_resources_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let impulse_dir = dir.path().to_path_buf();
        // Create a GENOME.md and config.json so they appear in listing.
        std::fs::write(impulse_dir.join("GENOME.md"), "# Test").unwrap();
        std::fs::write(impulse_dir.join("config.json"), "{}").unwrap();

        let server = McpServer::new(
            Arc::new(ToolRegistry::with_defaults()),
            ToolContext {
                impulse_dir,
                execution_origin: ExecutionOrigin::Test,
                ..ToolContext::with_all_capabilities()
            },
        );
        let response = server
            .process_request(r#"{"method":"resources/list"}"#)
            .await;
        let resources = response["resources"].as_array().unwrap();
        assert!(resources.len() >= 2);

        let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
        assert!(uris.contains(&"impulse://genome"));
        assert!(uris.contains(&"impulse://config"));
    }

    #[tokio::test]
    async fn test_mcp_resources_read_genome() {
        let dir = tempfile::TempDir::new().unwrap();
        let impulse_dir = dir.path().to_path_buf();
        std::fs::write(
            impulse_dir.join("GENOME.md"),
            "# My Decisions\n\n- Use Rust",
        )
        .unwrap();

        let server = McpServer::new(
            Arc::new(ToolRegistry::with_defaults()),
            ToolContext {
                impulse_dir,
                execution_origin: ExecutionOrigin::Test,
                ..ToolContext::with_all_capabilities()
            },
        );
        let response = server
            .process_request(r#"{"method":"resources/read","params":{"uri":"impulse://genome"}}"#)
            .await;
        let contents = response["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["uri"], "impulse://genome");
        assert_eq!(contents[0]["mimeType"], "text/markdown");
        assert!(contents[0]["text"]
            .as_str()
            .unwrap()
            .contains("My Decisions"));
    }

    /// Review finding: the genome resource served the stored JSON labelled
    /// `text/markdown`.
    #[tokio::test]
    async fn test_mcp_resources_read_a_json_genome_as_markdown() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut genome = crate::memory::Genome::new();
        genome.add_decision("Use Rust".into(), Some("speed".into()), Vec::new());
        std::fs::write(
            dir.path().join("GENOME.md"),
            serde_json::to_string_pretty(&genome).unwrap(),
        )
        .unwrap();
        let server = McpServer::new(
            Arc::new(ToolRegistry::with_defaults()),
            ToolContext {
                impulse_dir: dir.path().to_path_buf(),
                execution_origin: ExecutionOrigin::Test,
                ..ToolContext::with_all_capabilities()
            },
        );
        let response = server
            .process_request(r#"{"method":"resources/read","params":{"uri":"impulse://genome"}}"#)
            .await;
        let content = &response["contents"][0];
        assert_eq!(content["mimeType"], "text/markdown");
        let text = content["text"].as_str().unwrap();
        assert!(
            text.contains("Use Rust") && text.contains("speed"),
            "{text}"
        );
        assert!(!text.contains("\"decisions\""), "served raw JSON: {text}");
        assert_eq!(
            resource_body("GENOME.md", r#"{"patterns":[]}"#.to_string()).1,
            "application/json"
        );
    }

    #[tokio::test]
    async fn test_mcp_resources_read_unknown_uri() {
        let response = test_server()
            .process_request(
                r#"{"method":"resources/read","params":{"uri":"impulse://nonexistent"}}"#,
            )
            .await;
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown resource URI"));
    }

    #[tokio::test]
    async fn test_mcp_resources_read_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let impulse_dir = dir.path().to_path_buf();
        // Don't create GENOME.md — should get read error.

        let server = McpServer::new(
            Arc::new(ToolRegistry::with_defaults()),
            ToolContext {
                impulse_dir,
                execution_origin: ExecutionOrigin::Test,
                ..ToolContext::with_all_capabilities()
            },
        );
        let response = server
            .process_request(r#"{"method":"resources/read","params":{"uri":"impulse://genome"}}"#)
            .await;
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Failed to read"));
    }

    #[tokio::test]
    async fn test_mcp_resources_list_empty_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let server = McpServer::new(
            Arc::new(ToolRegistry::with_defaults()),
            ToolContext {
                impulse_dir: dir.path().to_path_buf(),
                execution_origin: ExecutionOrigin::Test,
                ..ToolContext::with_all_capabilities()
            },
        );
        let response = server
            .process_request(r#"{"method":"resources/list"}"#)
            .await;
        assert_eq!(response["resources"].as_array().unwrap().len(), 0);
    }
}
