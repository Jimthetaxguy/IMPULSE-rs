use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::RwLock;

use crate::injection::{run_injection, InjectionMode, InjectionSurface};
use crate::llm_backends::{AnthropicProvider, ChatRequest, LlmProvider, Message, Role};
use crate::state::SharedState;

const SOCKET_NAME: &str = "impulse.sock";

pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub state: SharedState,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonRequest {
    Ping,
    Status,
    CreateSession {
        name: String,
        platform: Option<String>,
    },
    EndSession {
        session_id: String,
        summary: String,
    },
    TrackFile {
        session_id: String,
        file_path: String,
    },
    TrackTool {
        session_id: String,
        tool_name: String,
    },
    GetSession {
        session_id: String,
    },
    ListSessions,
    Chat {
        session_id: String,
        message: String,
        #[serde(default)]
        inject_mode: Option<String>,
        #[serde(default)]
        inject_explain: bool,
    },
    StewardStatus,
    StewardProposals {
        action: String,
        id: Option<String>,
    },
    StewardMemory,
    /// List all available tools (for agent discovery)
    ListTools {
        #[serde(default)]
        category: Option<String>,
    },
    /// Get a tool's descriptor (params, capabilities)
    DescribeTool {
        name: String,
    },
    /// Invoke a tool by name with JSON params
    InvokeTool {
        name: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Export tool schemas in Claude tool-calling format
    ToolSchema,
    /// Request AI coordination assistance via the Impulse Agent
    AgentAssist {
        prompt: String,
        context: Option<String>,
    },
    /// Evaluate an action against guardrail rules
    GuardEvaluate {
        target: String,
        action: String,
    },
    /// List active guardrail rules
    GuardList,
    /// Check if a file is being modified by another session
    CheckConflict {
        session_id: String,
        file_path: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonResponse {
    Ok {
        result: serde_json::Value,
    },
    Error {
        message: String,
    },
    AgentAssistResult {
        success: bool,
        response: String,
    },
    ConflictCheck {
        has_conflict: bool,
        conflicting_sessions: Vec<String>,
    },
}

pub struct Daemon {
    config: DaemonConfig,
    shutdown_flag: Arc<RwLock<bool>>,
    tool_registry: Arc<crate::tooling::ToolRegistry>,
}

impl Daemon {
    pub fn new(state: SharedState) -> Self {
        let socket_path = state
            .storage()
            .base_path()
            .join("sockets")
            .join(SOCKET_NAME);

        Self {
            config: DaemonConfig {
                socket_path: socket_path.clone(),
                state,
            },
            shutdown_flag: Arc::new(RwLock::new(false)),
            tool_registry: Arc::new(crate::tooling::ToolRegistry::with_defaults()),
        }
    }

    #[allow(dead_code)]
    pub fn socket_path(&self) -> &PathBuf {
        &self.config.socket_path
    }

    pub async fn start(&self) -> Result<()> {
        let socket_dir = self
            .config
            .socket_path
            .parent()
            .context("Invalid socket path")?;
        tokio::fs::create_dir_all(socket_dir)
            .await
            .context("Failed to create socket directory")?;

        if self.config.socket_path.exists() {
            tokio::fs::remove_file(&self.config.socket_path)
                .await
                .context("Failed to remove old socket")?;
        }

        let listener =
            UnixListener::bind(&self.config.socket_path).context("Failed to bind socket")?;

        println!("Daemon listening on {}", self.config.socket_path.display());

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let state = self.config.state.clone();
                            let shutdown = self.shutdown_flag.clone();
                            let registry = self.tool_registry.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, state, shutdown, registry).await {
                                    eprintln!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Accept error: {}", e);
                        }
                    }
                }
                _ = self.check_shutdown() => {
                    println!("Shutting down daemon...");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn check_shutdown(&self) {
        loop {
            if let Ok(flag) = self.shutdown_flag.try_read() {
                if *flag {
                    return;
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        if let Ok(mut flag) = self.shutdown_flag.try_write() {
            *flag = true;
        }

        if self.config.socket_path.exists() {
            let _ = tokio::fs::remove_file(&self.config.socket_path).await;
        }
    }
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    state: SharedState,
    shutdown: Arc<RwLock<bool>>,
    registry: Arc<crate::tooling::ToolRegistry>,
) -> Result<()> {
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024; // 10MB limit per request
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        if line.len() > MAX_REQUEST_SIZE {
            let err_response = DaemonResponse::Error {
                message: format!(
                    "Request too large ({} bytes, max {})",
                    line.len(),
                    MAX_REQUEST_SIZE
                ),
            };
            let response_json = serde_json::to_string(&err_response)?;
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            line.clear();
            continue;
        }
        let request: DaemonRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let response = DaemonResponse::Error {
                    message: format!("Failed to parse request: {}", e),
                };
                writer
                    .write_all(serde_json::to_string(&response)?.as_bytes())
                    .await?;
                writer.write_all(b"\n").await?;
                line.clear();
                continue;
            }
        };

        let response = process_request(request, state.clone(), &registry).await;

        writer
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        line.clear();

        if let Ok(flag) = shutdown.try_read() {
            if *flag {
                break;
            }
        }
    }

    Ok(())
}

async fn process_request(
    request: DaemonRequest,
    state: SharedState,
    registry: &crate::tooling::ToolRegistry,
) -> DaemonResponse {
    match request {
        DaemonRequest::Ping => DaemonResponse::Ok {
            result: serde_json::json!({"pong": true}),
        },

        DaemonRequest::Status => match state.list_sessions().await {
            Ok(sessions) => DaemonResponse::Ok {
                result: serde_json::json!({
                    "sessions": sessions.len(),
                    "active": sessions.iter().filter(|s| s.status == crate::state::SessionStatus::Active).count()
                }),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::CreateSession { name, platform } => {
            let platform = platform.and_then(|p| match p.as_str() {
                "claude-code" => Some(crate::state::Platform::ClaudeCode),
                "opencode" => Some(crate::state::Platform::OpenCode),
                _ => None,
            });

            match state.create_session(name, platform).await {
                Ok(session) => DaemonResponse::Ok {
                    result: serde_json::json!({
                        "session_id": session.id,
                        "name": session.name
                    }),
                },
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }

        DaemonRequest::EndSession {
            session_id,
            summary,
        } => match state.end_session(&session_id, summary).await {
            Ok(Some(entry)) => DaemonResponse::Ok {
                result: serde_json::json!({
                    "session_id": entry.session_id,
                    "ended_at": entry.ended_at
                }),
            },
            Ok(None) => DaemonResponse::Error {
                message: "Session not found".to_string(),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::TrackFile {
            session_id,
            file_path,
        } => match state.track_file(&session_id, &file_path).await {
            Ok(_) => DaemonResponse::Ok {
                result: serde_json::json!({"tracked": true}),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::TrackTool {
            session_id,
            tool_name,
        } => match state.track_tool(&session_id, &tool_name).await {
            Ok(_) => DaemonResponse::Ok {
                result: serde_json::json!({"tracked": true}),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::CheckConflict {
            session_id,
            file_path,
        } => match state.check_file_conflict(&session_id, &file_path).await {
            Ok(conflicting) => DaemonResponse::ConflictCheck {
                has_conflict: !conflicting.is_empty(),
                conflicting_sessions: conflicting,
            },
            Err(e) => DaemonResponse::Error {
                message: format!("Conflict check failed: {}", e),
            },
        },

        DaemonRequest::GetSession { session_id } => match state.get_session(&session_id).await {
            Ok(Some(session)) => match serde_json::to_value(session) {
                Ok(result) => DaemonResponse::Ok { result },
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to serialize session: {}", e),
                },
            },
            Ok(None) => DaemonResponse::Error {
                message: "Session not found".to_string(),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::ListSessions => match state.list_sessions().await {
            Ok(sessions) => match serde_json::to_value(sessions) {
                Ok(result) => DaemonResponse::Ok { result },
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to serialize sessions: {}", e),
                },
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::Chat {
            session_id,
            message,
            inject_mode,
            inject_explain,
        } => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to read config: {}", e),
                    }
                }
            };
            let api_key = std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("CLAUDE_API_KEY"))
                .unwrap_or_else(|_| "".to_string());

            #[cfg(debug_assertions)]
            let test_mode = std::env::var("IMPULSE_TEST_MODE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            #[cfg(not(debug_assertions))]
            let test_mode = false;

            if api_key.is_empty() && !test_mode {
                return DaemonResponse::Error {
                    message: "ANTHROPIC_API_KEY or CLAUDE_API_KEY not set".to_string(),
                };
            }

            // Build session context from state
            let session_context = state.get_session(&session_id).await.ok().flatten();

            // Create context prompt with session history
            let mut context_prompt = if let Some(session) = &session_context {
                let files_list = session.active_files.join(", ");
                let tools_list = session.recent_tools.join(", ");

                format!(
                    "Session Context:\n- Session: {} (ID: {})\n- Files touched: {}\n- Recent tools: {}\n\nUser question: {}",
                    session.name, session.id,
                    if files_list.is_empty() { "none".to_string() } else { files_list },
                    if tools_list.is_empty() { "none".to_string() } else { tools_list },
                    message
                )
            } else {
                message.clone()
            };

            let mode_override = inject_mode.as_deref().and_then(InjectionMode::parse);
            if inject_mode.is_some() && mode_override.is_none() {
                return DaemonResponse::Error {
                    message: "Invalid inject_mode. Use off|review|apply".to_string(),
                };
            }
            let mut injection_query_parts = vec![message.clone()];
            if let Some(session) = &session_context {
                injection_query_parts.push(session.name.clone());
                if !session.active_files.is_empty() {
                    injection_query_parts.push(session.active_files.join(" "));
                }
                if !session.recent_tools.is_empty() {
                    injection_query_parts.push(session.recent_tools.join(" "));
                }
            }

            let injection_result = run_injection(
                state.storage().base_path(),
                &config,
                InjectionSurface::DaemonChat,
                mode_override,
                &injection_query_parts,
            );

            if injection_result.applied {
                if let Some(block) = &injection_result.injected_block {
                    context_prompt = format!("{}\n\n{}", block, context_prompt);
                }
            }

            if test_mode {
                return DaemonResponse::Ok {
                    result: serde_json::json!({
                        "response": format!("TEST_MODE_RESPONSE: {}", message),
                        "session_id": session_id,
                        "model": "test-mode",
                        "context_included": session_context.is_some(),
                        "injection": if inject_explain {
                            serde_json::to_value(&injection_result).unwrap_or_else(|_| serde_json::json!({"status": "serialization_error"}))
                        } else {
                            serde_json::json!({
                                "requested_mode": injection_result.requested_mode,
                                "effective_mode": injection_result.effective_mode,
                                "applied": injection_result.applied,
                                "artifact_path": injection_result.artifact_path,
                                "fallback_code": injection_result.explain.fallback_code,
                            })
                        }
                    }),
                };
            }

            let provider = AnthropicProvider::new(api_key);
            let model = std::env::var("IMPULSE_MODEL")
                .or_else(|_| std::env::var("COCKPIT_MODEL"))
                .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
            let request = ChatRequest {
                model,
                messages: vec![Message {
                    role: Role::User,
                    content: context_prompt,
                }],
                temperature: 0.7,
                max_tokens: Some(4096),
            };

            match provider.chat(request).await {
                Ok(response) => DaemonResponse::Ok {
                    result: serde_json::json!({
                        "response": response.content,
                        "session_id": session_id,
                        "model": response.model,
                        "context_included": session_context.is_some(),
                        "injection": if inject_explain {
                            serde_json::to_value(&injection_result).unwrap_or_else(|_| serde_json::json!({"status": "serialization_error"}))
                        } else {
                            serde_json::json!({
                                "requested_mode": injection_result.requested_mode,
                                "effective_mode": injection_result.effective_mode,
                                "applied": injection_result.applied,
                                "artifact_path": injection_result.artifact_path,
                                "fallback_code": injection_result.explain.fallback_code,
                            })
                        }
                    }),
                },
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }

        DaemonRequest::StewardStatus => {
            use crate::stewardship;

            let base = state.storage().base_path();
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to read config: {}", e),
                    }
                }
            };
            let stew_config = stewardship::StewardshipConfig::from_config(&config);

            let proposals = stewardship::approval::list_pending(base).unwrap_or_default();
            let cross = stewardship::cross_project::load_cross_project(base).unwrap_or_default();

            DaemonResponse::Ok {
                result: serde_json::json!({
                    "mode": stew_config.mode.as_str(),
                    "thresholds": {
                        "monitor": stew_config.monitor_threshold,
                        "surgical": stew_config.surgical_threshold,
                        "thoughtful": stew_config.thoughtful_threshold,
                        "emergency": stew_config.emergency_threshold,
                    },
                    "context_window_tokens": stew_config.context_window_tokens,
                    "pending_proposals": proposals.len(),
                    "cross_project_patterns": cross.patterns.len(),
                    "cross_project_learnings": cross.learnings.len(),
                }),
            }
        }

        DaemonRequest::StewardProposals { action, id } => {
            use crate::stewardship;

            let base = state.storage().base_path();

            match action.as_str() {
                "list" => match stewardship::approval::list_pending(base) {
                    Ok(proposals) => {
                        let out: Vec<_> = proposals
                            .iter()
                            .map(|p| {
                                serde_json::json!({
                                    "id": p.id,
                                    "strategy": p.strategy.as_str(),
                                    "threshold": p.threshold.as_str(),
                                    "estimated_tokens_freed": p.estimated_tokens_freed,
                                    "regions": p.regions.len(),
                                })
                            })
                            .collect();
                        DaemonResponse::Ok {
                            result: serde_json::json!(out),
                        }
                    }
                    Err(e) => DaemonResponse::Error {
                        message: e.to_string(),
                    },
                },
                "approve" => {
                    let pid = match id {
                        Some(pid) => pid,
                        None => {
                            return DaemonResponse::Error {
                                message: "id required for approve".to_string(),
                            }
                        }
                    };
                    match stewardship::approval::approve_proposal(base, &pid) {
                        Ok(true) => DaemonResponse::Ok {
                            result: serde_json::json!({"approved": pid}),
                        },
                        Ok(false) => DaemonResponse::Error {
                            message: format!("Proposal {} not found", pid),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                "reject" => {
                    let pid = match id {
                        Some(pid) => pid,
                        None => {
                            return DaemonResponse::Error {
                                message: "id required for reject".to_string(),
                            }
                        }
                    };
                    match stewardship::approval::reject_proposal(base, &pid) {
                        Ok(true) => DaemonResponse::Ok {
                            result: serde_json::json!({"rejected": pid}),
                        },
                        Ok(false) => DaemonResponse::Error {
                            message: format!("Proposal {} not found", pid),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                _ => DaemonResponse::Error {
                    message: format!("Unknown action: {}. Use list, approve, reject", action),
                },
            }
        }

        DaemonRequest::ListTools { category } => {
            let descriptors = if let Some(cat) = category {
                let cat = match cat.as_str() {
                    "system" => crate::tooling::ToolCategory::System,
                    "utility" => crate::tooling::ToolCategory::Utility,
                    "analysis" => crate::tooling::ToolCategory::Analysis,
                    "document" => crate::tooling::ToolCategory::Document,
                    _ => {
                        return DaemonResponse::Error {
                            message: format!(
                                "Unknown category: {}. Use system, utility, analysis, document",
                                cat
                            ),
                        }
                    }
                };
                registry.list_by_category(cat)
            } else {
                registry.list()
            };
            let out: Vec<_> = descriptors
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "name": d.name,
                        "description": d.description,
                        "category": format!("{}", d.category),
                        "params": d.params.len(),
                    })
                })
                .collect();
            DaemonResponse::Ok {
                result: serde_json::json!({"tools": out, "count": out.len()}),
            }
        }

        DaemonRequest::DescribeTool { name } => match registry.get(&name) {
            Some(tool) => {
                let desc = tool.descriptor();
                DaemonResponse::Ok {
                    result: serde_json::json!({
                        "id": desc.id,
                        "name": desc.name,
                        "description": desc.description,
                        "version": desc.version,
                        "category": format!("{}", desc.category),
                        "params": desc.params.iter().map(|p| serde_json::json!({
                            "name": p.name,
                            "description": p.description,
                            "type": p.param_type.as_str(),
                            "required": p.required,
                            "default": p.default,
                        })).collect::<Vec<_>>(),
                        "capabilities": tool.required_capabilities().iter()
                            .map(|c| c.as_str())
                            .collect::<Vec<_>>(),
                    }),
                }
            }
            None => DaemonResponse::Error {
                message: format!("Tool not found: {}", name),
            },
        },

        DaemonRequest::InvokeTool { name, params } => {
            // Use restricted context for remote callers (deny-by-default security)
            let ctx = crate::tooling::ToolContext::default();
            match registry.execute(&name, params, &ctx).await {
                Ok(result) => DaemonResponse::Ok {
                    result: serde_json::json!({
                        "tool": name,
                        "output": result.output,
                    }),
                },
                Err(e) => DaemonResponse::Error {
                    message: format!("{}", e),
                },
            }
        }

        DaemonRequest::ToolSchema => {
            let tools: Vec<_> = registry
                .list()
                .iter()
                .map(|desc| {
                    let mut properties = serde_json::Map::new();
                    let mut required = Vec::new();
                    for param in &desc.params {
                        let json_type = match param.param_type {
                            crate::tooling::ParamType::String
                            | crate::tooling::ParamType::FilePath => "string",
                            crate::tooling::ParamType::Integer => "integer",
                            crate::tooling::ParamType::Float => "number",
                            crate::tooling::ParamType::Bool => "boolean",
                            crate::tooling::ParamType::Json => "object",
                        };
                        let mut prop = serde_json::json!({
                            "type": json_type,
                            "description": param.description,
                        });
                        if let Some(default) = &param.default {
                            prop["default"] = default.clone();
                        }
                        properties.insert(param.name.clone(), prop);
                        if param.required {
                            required.push(serde_json::Value::String(param.name.clone()));
                        }
                    }
                    serde_json::json!({
                        "name": desc.id,
                        "description": desc.description,
                        "input_schema": {
                            "type": "object",
                            "properties": properties,
                            "required": required,
                        }
                    })
                })
                .collect();
            DaemonResponse::Ok {
                result: serde_json::json!({"tools": tools}),
            }
        }

        DaemonRequest::StewardMemory => {
            use crate::stewardship;

            let base = state.storage().base_path();
            match stewardship::cross_project::load_cross_project(base) {
                Ok(cross) => DaemonResponse::Ok {
                    result: serde_json::json!({
                        "version": cross.version,
                        "updated": cross.updated.to_rfc3339(),
                        "patterns": cross.patterns.iter().map(|p| serde_json::json!({
                            "id": p.id,
                            "type": p.pattern_type,
                            "description": p.description,
                            "occurrences": p.occurrences,
                            "projects": p.projects,
                            "insight": p.insight,
                        })).collect::<Vec<_>>(),
                        "learnings": cross.learnings,
                        "stats": {
                            "total_patterns": cross.stats.total_patterns,
                            "total_sessions": cross.stats.total_sessions_analyzed,
                            "total_learnings": cross.stats.total_learnings,
                        },
                    }),
                },
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }

        DaemonRequest::AgentAssist { prompt, context } => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::AgentAssistResult {
                        success: false,
                        response: format!("Failed to load config: {}", e),
                    }
                }
            };

            let mut agent = match crate::agent::resolve_from_config(
                config.impulse_agent_provider.as_deref(),
                config.impulse_agent_api_key.as_deref(),
                config.impulse_agent_model.as_deref(),
                config.impulse_agent_harness.as_deref(),
            ) {
                Some(a) => a,
                None => {
                    return DaemonResponse::AgentAssistResult {
                        success: false,
                        response: "Impulse Agent not configured. Run: impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY".to_string(),
                    }
                }
            };

            if !agent.is_ready() {
                return DaemonResponse::AgentAssistResult {
                    success: false,
                    response: "Impulse Agent is configured but not ready (check API key or harness installation)".to_string(),
                };
            }

            // Build the full prompt, incorporating optional context
            let full_prompt = match context {
                Some(ctx) => format!("Context:\n{}\n\nRequest:\n{}", ctx, prompt),
                None => prompt,
            };

            match agent
                .query(crate::agent::prompts::COORDINATION_SYSTEM, &full_prompt)
                .await
            {
                Ok(response) => DaemonResponse::AgentAssistResult {
                    success: true,
                    response,
                },
                Err(e) => DaemonResponse::AgentAssistResult {
                    success: false,
                    response: format!("Agent query failed: {}", e),
                },
            }
        }

        DaemonRequest::GuardEvaluate { target, action } => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to read config: {}", e),
                    }
                }
            };
            match crate::guardrail::evaluate_action(&action, &target, &config.guardrails) {
                Ok(results) => {
                    let has_block = crate::guardrail::GuardEngine::has_blocking(&results);
                    DaemonResponse::Ok {
                        result: serde_json::json!({
                            "blocked": has_block,
                            "results": results,
                        }),
                    }
                }
                Err(e) => DaemonResponse::Error {
                    message: format!("Guardrail evaluation failed: {}", e),
                },
            }
        }

        DaemonRequest::GuardList => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to read config: {}", e),
                    }
                }
            };
            let rules = crate::guardrail::list_active_rules(&config.guardrails);
            DaemonResponse::Ok {
                result: serde_json::json!({ "rules": rules }),
            }
        }
    }
}

#[cfg(test)]
mod tests;
