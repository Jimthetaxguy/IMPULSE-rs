// MCP Server implementation for Impulse
// Provides JSON-RPC interface for external agents

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::plugin::registry::{global_registry, PluginRegistry};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum McpRequest {
    #[serde(rename = "tools/list")]
    ListTools,

    #[serde(rename = "tools/call")]
    CallTool {
        name: String,
        arguments: serde_json::Value,
    },

    #[serde(rename = "resources/list")]
    ListResources,

    #[serde(rename = "resources/read")]
    ReadResource { uri: String },

    #[serde(rename = "plugins/list")]
    ListPlugins,

    #[serde(rename = "plugins/execute")]
    ExecutePlugin {
        plugin: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpResponse {
    #[serde(rename = "tool")]
    Tool {
        name: String,
        description: String,
        input_schema: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult { content: Vec<serde_json::Value> },

    #[serde(rename = "resource")]
    Resource {
        uri: String,
        mime_type: String,
        content: String,
    },

    #[serde(rename = "plugin")]
    Plugin {
        name: String,
        version: String,
        category: String,
    },

    #[serde(rename = "plugin_result")]
    PluginResult { result: serde_json::Value },

    #[serde(rename = "error")]
    Error { code: i32, message: String },
}

pub struct McpServer {
    registry: Arc<PluginRegistry>,
    port: u16,
}

impl McpServer {
    pub fn new(port: u16) -> Self {
        Self {
            registry: Arc::new(global_registry().clone()),
            port,
        }
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("127.0.0.1:{}", self.port);
        let listener = TcpListener::bind(&addr).await?;

        println!("MCP server listening on {}", addr);

        loop {
            let (socket, _) = listener.accept().await?;
            let registry = self.registry.clone();

            tokio::spawn(async move {
                if let Err(e) = Self::handle_connection(socket, registry).await {
                    eprintln!("MCP connection error: {}", e);
                }
            });
        }
    }

    async fn handle_connection(
        socket: tokio::net::TcpStream,
        registry: Arc<PluginRegistry>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (reader, mut writer) = socket.into_split();
        let mut reader = BufReader::new(reader);
        let mut buffer = String::new();

        while let Ok(n) = reader.read_line(&mut buffer).await {
            if n == 0 {
                break;
            }

            let response = Self::process_request(&buffer, &registry);
            let json_str = serde_json::to_string(&response)?;
            writer.write_all(json_str.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            buffer.clear();
        }

        Ok(())
    }

    fn process_request(request_str: &str, registry: &PluginRegistry) -> serde_json::Value {
        let request: Result<serde_json::Value, _> = serde_json::from_str(request_str);

        match request {
            Ok(req) => {
                let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

                match method {
                    "tools/list" => {
                        let tools = vec![
                            serde_json::json!({
                                "name": "impulse_parse_document",
                                "description": "Parse an Office document and extract content",
                                "input_schema": {
                                    "type": "object",
                                    "properties": {
                                        "path": {"type": "string", "description": "Path to document"}
                                    },
                                    "required": ["path"]
                                }
                            }),
                            serde_json::json!({
                                "name": "impulse_extract_smart",
                                "description": "Extract intelligent content from document using query",
                                "input_schema": {
                                    "type": "object",
                                    "properties": {
                                        "path": {"type": "string"},
                                        "query": {"type": "string"}
                                    },
                                    "required": ["path", "query"]
                                }
                            }),
                            serde_json::json!({
                                "name": "impulse_list_plugins",
                                "description": "List available Impulse plugins",
                                "input_schema": {
                                    "type": "object",
                                    "properties": {}
                                }
                            }),
                        ];
                        serde_json::json!({"tools": tools})
                    }
                    "tools/call" => {
                        let name = req
                            .get("arguments")
                            .and_then(|a| a.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");

                        let path = req
                            .get("arguments")
                            .and_then(|a| a.get("path"))
                            .and_then(|p| p.as_str())
                            .map(std::path::PathBuf::from);

                        if name == "impulse_parse_document" {
                            if let Some(path) = path {
                                match registry.extract(&path) {
                                    Ok(result) => {
                                        serde_json::json!({
                                            "content": [{"type": "text", "text": result.content}]
                                        })
                                    }
                                    Err(e) => {
                                        serde_json::json!({
                                            "content": [{"type": "text", "text": format!("Error: {}", e)}],
                                            "isError": true
                                        })
                                    }
                                }
                            } else {
                                serde_json::json!({
                                    "content": [{"type": "text", "text": "Missing path parameter"}],
                                    "isError": true
                                })
                            }
                        } else {
                            serde_json::json!({
                                "content": [{"type": "text", "text": "Unknown tool"}],
                                "isError": true
                            })
                        }
                    }
                    "plugins/list" => {
                        let plugins = registry.list_context_providers();
                        let result: Vec<_> = plugins
                            .iter()
                            .map(|p| {
                                serde_json::json!({
                                    "name": p.name,
                                    "version": p.version,
                                    "category": p.category.as_str(),
                                    "formats": p.supported_formats
                                })
                            })
                            .collect();
                        serde_json::json!({"plugins": result})
                    }
                    _ => {
                        serde_json::json!({
                            "error": {"code": -32601, "message": "Method not found"}
                        })
                    }
                }
            }
            Err(e) => {
                serde_json::json!({
                    "error": {"code": -32700, "message": format!("Parse error: {}", e)}
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_request_parsing() {
        let json = r#"{"method": "tools/list"}"#;
        let req: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(req["method"], "tools/list");
    }

    #[test]
    fn test_mcp_tool_response() {
        let response = McpResponse::Tool {
            name: "test".to_string(),
            description: "Test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test"));
    }
}
