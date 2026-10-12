//! CLI handlers for the ElevenLabs-first voice engine.

use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::Path;

use crate::cli::VoiceCommands;
use crate::voice::{
    default_voice_provider, elevenlabs_client_tool_schemas, ensure_elevenlabs_env,
    invoke_elevenlabs_client_tool, parse_webhook_tool_request, voice_engine_docs,
    ElevenLabsClientToolRequest, SecretSource, VoiceServer, VoiceToolBridge, VoiceTransport,
    DEFAULT_VOICE_EXPOSED_TOOLS,
};

/// Dispatch `impulse-rs voice …` subcommands.
pub async fn handle_voice(_impulse_dir: &Path, subcommand: VoiceCommands) -> Result<()> {
    // Best-effort: load ElevenLabs_API_Key from Infisical into env (never log value).
    let (key_ok, key_source) = ensure_elevenlabs_env();

    match subcommand {
        VoiceCommands::Status { json } => {
            let provider = default_voice_provider();
            let source_label = match &key_source {
                SecretSource::Env => "env:ELEVENLABS_API_KEY",
                SecretSource::Infisical { secret_name } => {
                    // Keep name only — no value.
                    // Infisical vault key on this machine is typically ElevenLabs_API_Key.
                    secret_name.as_str()
                }
                SecretSource::Missing => "missing",
            };
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "primary_provider": provider.as_str(),
                        "is_primary": provider.is_primary(),
                        "backend": "elevenlabs_conversational_agent",
                        "tool_bridge": "ToolRegistry::execute",
                        "api_key_present": key_ok,
                        "api_key_source": match &key_source {
                            SecretSource::Env => "env",
                            SecretSource::Infisical { .. } => "infisical",
                            SecretSource::Missing => "missing",
                        },
                        "api_key_source_detail": source_label,
                        "docs": "impulse-rs voice docs",
                    })
                );
            } else {
                println!(
                    "Voice provider: {} (primary={})",
                    provider.as_str(),
                    provider.is_primary()
                );
                println!("Backend: ElevenLabs Conversational Agent (client tools / webhooks)");
                println!("Tool bridge: real ToolRegistry::execute (not a parallel registry)");
                println!(
                    "API key: {} ({})",
                    if key_ok { "present" } else { "missing" },
                    source_label
                );
                println!("Docs: impulse-rs voice docs");
            }
            Ok(())
        }
        VoiceCommands::ListTools { json } => {
            let bridge = VoiceToolBridge::with_defaults();
            let registry = bridge.registry();
            let mut rows = Vec::new();
            for id in DEFAULT_VOICE_EXPOSED_TOOLS {
                let risk = registry
                    .get(id)
                    .map(crate::voice::classify_tool_risk)
                    .map(|r| format!("{r:?}"))
                    .unwrap_or_else(|| "Unknown".into());
                let desc = registry
                    .get(id)
                    .map(|t| t.descriptor().description)
                    .unwrap_or_default();
                rows.push(serde_json::json!({
                    "id": id,
                    "risk": risk,
                    "description": desc,
                }));
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                println!("Voice-exposed tools (mutating require --confirmed):\n");
                for row in &rows {
                    println!(
                        "  {} [{}] — {}",
                        row["id"].as_str().unwrap_or("?"),
                        row["risk"].as_str().unwrap_or("?"),
                        row["description"].as_str().unwrap_or("")
                    );
                }
            }
            Ok(())
        }
        VoiceCommands::ToolCall {
            name,
            params,
            tool_call_id,
            confirmed,
            stdin,
            json,
        } => {
            let request = if stdin {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .context("failed to read stdin for voice tool-call")?;
                // Accept either client-tool or webhook-shaped JSON.
                match serde_json::from_str::<ElevenLabsClientToolRequest>(&buf) {
                    Ok(mut req) => {
                        if confirmed {
                            req.confirmed = true;
                        }
                        req
                    }
                    Err(_) => {
                        let mut req = parse_webhook_tool_request(buf.as_bytes())
                            .map_err(|e| anyhow::anyhow!(e))?;
                        if confirmed {
                            req.confirmed = true;
                        }
                        req
                    }
                }
            } else {
                let tool = name.context("--name is required unless --stdin is set")?;
                let params_val: serde_json::Value =
                    serde_json::from_str(&params).context("invalid --params JSON")?;
                ElevenLabsClientToolRequest {
                    tool_call_id,
                    tool,
                    params: params_val,
                    confirmed,
                    wait_for_response: true,
                    source: crate::voice::VoiceToolCallSource::ClientTool,
                }
            };

            let result = invoke_elevenlabs_client_tool(request).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status: {:?}", result.status);
                println!("tool: {}", result.tool);
                println!("provider: {}", result.provider);
                if let Some(err) = &result.error {
                    println!("error: {err}");
                }
                println!("result: {}", result.result);
            }

            match result.status {
                crate::voice::ElevenLabsToolResultStatus::Ok => Ok(()),
                crate::voice::ElevenLabsToolResultStatus::Denied => {
                    bail!(
                        "voice tool denied: {}",
                        result.error.unwrap_or_else(|| "denied".into())
                    )
                }
                crate::voice::ElevenLabsToolResultStatus::Error => {
                    bail!(
                        "voice tool error: {}",
                        result.error.unwrap_or_else(|| "error".into())
                    )
                }
            }
        }
        VoiceCommands::Docs => {
            print!("{}", voice_engine_docs());
            Ok(())
        }
        VoiceCommands::Schema { json: _json } => {
            let bridge = VoiceToolBridge::with_defaults();
            let schemas = elevenlabs_client_tool_schemas(bridge.registry(), bridge.policy());
            // Always JSON — this is the agent registration payload.
            println!("{}", serde_json::to_string_pretty(&schemas)?);
            Ok(())
        }
        VoiceCommands::Serve { transport, port } => {
            let server = VoiceServer::with_defaults();
            let t = match transport.to_ascii_lowercase().as_str() {
                "stdio" => VoiceTransport::Stdio,
                "tcp" => VoiceTransport::Tcp(port),
                "webhook" | "http" => VoiceTransport::Webhook(port),
                other => bail!("unknown voice transport `{other}` (use stdio, tcp, or webhook)"),
            };
            server.serve(t).await.context("voice serve failed")?;
            Ok(())
        }
    }
}
