//! Agent backend — subprocess (claude --print) and direct API (ureq) modes.
//!
//! Each query runs on a background `std::thread`. Results return via `mpsc::channel`.
//! No async runtime needed.

use std::sync::mpsc;

use crate::state::{ConnectionStatus, StateHandle};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Response from an agent query.
pub struct AgentResponse {
    pub content: String,
    pub is_error: bool,
}

/// Which backend to use for agent queries.
#[derive(Clone)]
pub enum AgentBackend {
    /// Routes through the daemon's Chat endpoint (includes injection + context).
    DaemonChat,
    /// Uses `claude --print -p <prompt>` subprocess.
    Harness {
        /// Session ID for `--resume` continuity (if supported).
        session_id: Option<String>,
    },
    /// Uses direct HTTP API calls via ureq.
    Api {
        api_key: String,
        model: String,
        /// Conversation history: Vec<(role, content)>.
        history: Vec<(String, String)>,
    },
    /// No backend available — user needs to configure one.
    Unavailable,
}

impl AgentBackend {
    /// Detect the best available backend.
    ///
    /// Priority: claude in PATH > ANTHROPIC_API_KEY env var > Unavailable.
    /// Note: DaemonChat is added dynamically when the daemon is connected;
    /// it's not part of the static detection.
    pub fn detect() -> Self {
        if which::which("claude").is_ok() {
            return Self::Harness { session_id: None };
        }
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            if !key.is_empty() {
                return Self::Api {
                    api_key: key,
                    model: "claude-sonnet-4-20250514".to_string(),
                    history: Vec::new(),
                };
            }
        }
        Self::Unavailable
    }

    /// Human-readable label for the current backend.
    pub fn label(&self) -> &'static str {
        match self {
            Self::DaemonChat => "Daemon Chat",
            Self::Harness { .. } => "Claude Code",
            Self::Api { .. } => "API",
            Self::Unavailable => "Unavailable",
        }
    }
}

/// Resolve the effective backend at query time.
///
/// If the daemon is connected, prefer DaemonChat. Otherwise fall back
/// to the statically-detected backend.
pub fn resolve_backend(static_backend: &AgentBackend, state: &Option<StateHandle>) -> AgentBackend {
    if let Some(ref handle) = state {
        if let Ok(s) = handle.lock() {
            if s.connection == ConnectionStatus::Connected {
                return AgentBackend::DaemonChat;
            }
        }
    }
    static_backend.clone()
}

// ---------------------------------------------------------------------------
// Agent persona (system prompt)
// ---------------------------------------------------------------------------

/// The system prompt / persona for the Impulse Agent.
const AGENT_PERSONA: &str = "\
You are the Impulse Agent — a coordinator for AI coding sessions.

## Your Role
- Monitor and coordinate multiple AI coding agents working in terminal panes
- Detect file conflicts when two agents modify the same file
- Correlate errors across panes (one agent's change breaking another's work)
- Surface relevant context from session history and genome decisions
- Help the user manage their multi-agent workflow

## Guidelines
- Be concise. The user is watching worker terminals — don't demand attention unnecessarily.
- When you detect a conflict or error, state it clearly with the specific files and panes involved.
- Prefer actionable advice over general suggestions.
- You receive periodic context updates from worker panes via <impulse-context> blocks.";

// ---------------------------------------------------------------------------
// Subprocess (harness) backend
// ---------------------------------------------------------------------------

/// Run a query via `claude --print -p <prompt>` subprocess.
fn query_harness(prompt: &str, context: &str, session_id: Option<&str>) -> AgentResponse {
    let full_prompt = if context.is_empty() {
        prompt.to_string()
    } else {
        format!("{}\n\n{}", context, prompt)
    };

    let mut cmd = std::process::Command::new("claude");
    cmd.arg("--print").arg("-p").arg(&full_prompt);

    if let Some(sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }

    // Prevent the subprocess from inheriting our GUI's env vars.
    cmd.env_remove("CLAUDECODE");
    cmd.env_remove("CLAUDE_CODE_SESSION");

    match cmd.output() {
        Ok(output) if output.status.success() => {
            let content = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if content.is_empty() {
                AgentResponse {
                    content: "(empty response)".to_string(),
                    is_error: false,
                }
            } else {
                AgentResponse {
                    content,
                    is_error: false,
                }
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let msg = if stderr.is_empty() { stdout } else { stderr };
            AgentResponse {
                content: if msg.is_empty() {
                    format!("claude exited with status {}", output.status)
                } else {
                    msg
                },
                is_error: true,
            }
        }
        Err(e) => AgentResponse {
            content: format!("Failed to run claude: {}", e),
            is_error: true,
        },
    }
}

// ---------------------------------------------------------------------------
// API backend
// ---------------------------------------------------------------------------

/// Run a query via direct Anthropic API call using ureq.
fn query_api(
    api_key: &str,
    model: &str,
    history: &[(String, String)],
    user_message: &str,
    context: &str,
) -> AgentResponse {
    let system_prompt = if context.is_empty() {
        AGENT_PERSONA.to_string()
    } else {
        format!("{}\n\n{}", AGENT_PERSONA, context)
    };

    // Build messages array including history + new user message.
    let mut messages = Vec::new();
    for (role, content) in history {
        messages.push(serde_json::json!({"role": role, "content": content}));
    }
    messages.push(serde_json::json!({"role": "user", "content": user_message}));

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "system": system_prompt,
        "messages": messages,
    });

    let result = ureq::post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .send_json(&body);

    match result {
        Ok(response) => {
            let status = response.status();
            match response.into_body().read_to_string() {
                Ok(text) => {
                    if status == 200 {
                        parse_api_response(&text)
                    } else {
                        AgentResponse {
                            content: format!("API error ({}): {}", status, text),
                            is_error: true,
                        }
                    }
                }
                Err(e) => AgentResponse {
                    content: format!("Failed to read API response: {}", e),
                    is_error: true,
                },
            }
        }
        Err(e) => AgentResponse {
            content: format!("API request failed: {}", e),
            is_error: true,
        },
    }
}

/// Parse the Anthropic Messages API response JSON.
fn parse_api_response(body: &str) -> AgentResponse {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(json) => {
            // Extract text from content[0].text
            if let Some(content) = json.get("content").and_then(|c| c.as_array()) {
                let text: String = content
                    .iter()
                    .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");

                if text.is_empty() {
                    AgentResponse {
                        content: "(empty response)".to_string(),
                        is_error: false,
                    }
                } else {
                    AgentResponse {
                        content: text,
                        is_error: false,
                    }
                }
            } else if let Some(err) = json.get("error").and_then(|e| e.get("message")) {
                AgentResponse {
                    content: format!("API error: {}", err),
                    is_error: true,
                }
            } else {
                AgentResponse {
                    content: format!("Unexpected API response: {}", body),
                    is_error: true,
                }
            }
        }
        Err(e) => AgentResponse {
            content: format!("Failed to parse API response: {}", e),
            is_error: true,
        },
    }
}

// ---------------------------------------------------------------------------
// Daemon Chat backend
// ---------------------------------------------------------------------------

/// Run a query via the daemon's AgentAssist endpoint.
///
/// Creates a fresh `DaemonClient` per query — the client holds a `UnixStream`
/// which isn't `Send`, so we discover + connect on the background thread.
fn query_daemon_chat(prompt: &str, context: &str) -> AgentResponse {
    use crate::ipc::DaemonClient;

    let mut client = DaemonClient::discover();

    let ctx_opt = if context.is_empty() {
        None
    } else {
        Some(context)
    };

    match client.agent_assist(prompt, ctx_opt) {
        Ok(response) => {
            if response.is_empty() {
                AgentResponse {
                    content: "(empty response)".to_string(),
                    is_error: false,
                }
            } else {
                AgentResponse {
                    content: response,
                    is_error: false,
                }
            }
        }
        Err(e) => AgentResponse {
            content: format!("Daemon chat failed: {}", e),
            is_error: true,
        },
    }
}

// ---------------------------------------------------------------------------
// Background dispatch
// ---------------------------------------------------------------------------

/// Dispatch a query on a background thread. Returns a receiver for the response.
///
/// The thread runs the query synchronously and sends the result back via the channel.
/// The receiver should be polled with `try_recv()` in the GUI update loop.
pub fn dispatch_query(
    backend: &mut AgentBackend,
    prompt: &str,
    context: &str,
) -> mpsc::Receiver<AgentResponse> {
    let (tx, rx) = mpsc::channel();

    let prompt = prompt.to_string();
    let context = context.to_string();
    let backend_clone = backend.clone();

    // For API mode, add the user message to history before dispatching.
    if let AgentBackend::Api {
        ref mut history, ..
    } = backend
    {
        history.push(("user".to_string(), prompt.clone()));
    }

    std::thread::spawn(move || {
        let response = match &backend_clone {
            AgentBackend::DaemonChat => query_daemon_chat(&prompt, &context),
            AgentBackend::Harness { session_id } => {
                query_harness(&prompt, &context, session_id.as_deref())
            }
            AgentBackend::Api {
                api_key,
                model,
                history,
                ..
            } => query_api(api_key, model, history, &prompt, &context),
            AgentBackend::Unavailable => AgentResponse {
                content:
                    "No agent backend configured. Install Claude Code or set ANTHROPIC_API_KEY."
                        .to_string(),
                is_error: true,
            },
        };
        let _ = tx.send(response);
    });

    rx
}

/// After receiving a successful API response, update the backend's history.
pub fn record_api_response(backend: &mut AgentBackend, content: &str) {
    if let AgentBackend::Api {
        ref mut history, ..
    } = backend
    {
        history.push(("assistant".to_string(), content.to_string()));
        // Keep history bounded to last 20 turns.
        if history.len() > 40 {
            history.drain(..history.len() - 40);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_detect_returns_valid_variant() {
        let backend = AgentBackend::detect();
        // Should return some variant (which one depends on the test environment).
        let label = backend.label();
        assert!(
            label == "Claude Code" || label == "API" || label == "Unavailable",
            "Unexpected label: {}",
            label
        );
    }

    #[test]
    fn test_backend_label() {
        assert_eq!(AgentBackend::DaemonChat.label(), "Daemon Chat");
        assert_eq!(
            AgentBackend::Harness { session_id: None }.label(),
            "Claude Code"
        );
        assert_eq!(
            AgentBackend::Api {
                api_key: "test".into(),
                model: "test".into(),
                history: vec![]
            }
            .label(),
            "API"
        );
        assert_eq!(AgentBackend::Unavailable.label(), "Unavailable");
    }

    #[test]
    fn test_resolve_backend_without_state_returns_static() {
        let static_backend = AgentBackend::Harness { session_id: None };
        let resolved = resolve_backend(&static_backend, &None);
        assert_eq!(resolved.label(), "Claude Code");
    }

    #[test]
    fn test_resolve_backend_connected_returns_daemon_chat() {
        use crate::state::SharedState;
        use std::sync::{Arc, Mutex};

        let state: StateHandle = Arc::new(Mutex::new(SharedState::default()));
        // Set connected.
        state.lock().unwrap().connection = ConnectionStatus::Connected;

        let static_backend = AgentBackend::Harness { session_id: None };
        let resolved = resolve_backend(&static_backend, &Some(state));
        assert_eq!(resolved.label(), "Daemon Chat");
    }

    #[test]
    fn test_resolve_backend_disconnected_returns_static() {
        use crate::state::SharedState;
        use std::sync::{Arc, Mutex};

        let state: StateHandle = Arc::new(Mutex::new(SharedState::default()));
        // Default is Disconnected.
        let static_backend = AgentBackend::Api {
            api_key: "key".into(),
            model: "model".into(),
            history: vec![],
        };
        let resolved = resolve_backend(&static_backend, &Some(state));
        assert_eq!(resolved.label(), "API");
    }

    #[test]
    fn test_parse_api_response_success() {
        let json = r#"{"content":[{"type":"text","text":"Hello world"}]}"#;
        let resp = parse_api_response(json);
        assert!(!resp.is_error);
        assert_eq!(resp.content, "Hello world");
    }

    #[test]
    fn test_parse_api_response_error() {
        let json = r#"{"error":{"message":"Invalid API key"}}"#;
        let resp = parse_api_response(json);
        assert!(resp.is_error);
        assert!(resp.content.contains("Invalid API key"));
    }

    #[test]
    fn test_parse_api_response_empty_content() {
        let json = r#"{"content":[]}"#;
        let resp = parse_api_response(json);
        assert!(!resp.is_error);
        assert_eq!(resp.content, "(empty response)");
    }

    #[test]
    fn test_parse_api_response_invalid_json() {
        let resp = parse_api_response("not json");
        assert!(resp.is_error);
        assert!(resp.content.contains("Failed to parse"));
    }

    #[test]
    fn test_record_api_response_appends() {
        let mut backend = AgentBackend::Api {
            api_key: "key".into(),
            model: "model".into(),
            history: vec![("user".into(), "hello".into())],
        };
        record_api_response(&mut backend, "response");
        if let AgentBackend::Api { history, .. } = &backend {
            assert_eq!(history.len(), 2);
            assert_eq!(history[1].0, "assistant");
            assert_eq!(history[1].1, "response");
        } else {
            panic!("Expected Api backend");
        }
    }

    #[test]
    fn test_record_api_response_bounds_history() {
        let mut backend = AgentBackend::Api {
            api_key: "key".into(),
            model: "model".into(),
            history: (0..45)
                .map(|i| ("user".into(), format!("msg{}", i)))
                .collect(),
        };
        record_api_response(&mut backend, "new");
        if let AgentBackend::Api { history, .. } = &backend {
            assert!(history.len() <= 41); // 45 + 1 = 46, drained to 40
        } else {
            panic!("Expected Api backend");
        }
    }

    #[test]
    fn test_dispatch_unavailable_returns_error() {
        let mut backend = AgentBackend::Unavailable;
        let rx = dispatch_query(&mut backend, "test", "");
        let resp = rx.recv().expect("Should receive response");
        assert!(resp.is_error);
        assert!(resp.content.contains("No agent backend configured"));
    }

    #[test]
    fn test_agent_persona_is_nonempty() {
        assert!(!AGENT_PERSONA.is_empty());
        assert!(AGENT_PERSONA.contains("Impulse Agent"));
    }
}
