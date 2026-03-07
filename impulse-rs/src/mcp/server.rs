use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

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

    async fn serve_stdio(&self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut writer = tokio::io::BufWriter::new(stdout);
        let mut line = String::new();

        while reader.read_line(&mut line).await? > 0 {
            let response = self.process_request(&line).await;
            writer
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            line.clear();
        }

        Ok(())
    }

    async fn serve_tcp(&self, port: u16) -> Result<()> {
        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        println!("MCP compatibility server listening on {}", addr);

        loop {
            let (socket, _) = listener.accept().await?;
            let registry = self.registry.clone();
            let ctx = self.ctx.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = socket.into_split();
                let mut reader = BufReader::new(reader);
                let server = McpServer::new(registry, ctx);
                let mut line = String::new();

                loop {
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
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
                            line.clear();
                        }
                        Err(_) => break,
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
            "resources/list" => serde_json::json!({ "resources": [] }),
            "resources/read" => serde_json::json!({
                "error": {"code": -32601, "message": "resources/read is not implemented"}
            }),
            _ => serde_json::json!({
                "error": {"code": -32601, "message": "Method not found"}
            }),
        }
    }
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
    use crate::tooling::{ExecutionOrigin, ToolRegistry};

    fn test_server() -> McpServer {
        McpServer::new(
            Arc::new(ToolRegistry::with_defaults()),
            ToolContext {
                execution_origin: ExecutionOrigin::Test,
                ..ToolContext::with_all_capabilities()
            },
        )
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
    async fn test_mcp_tool_response() {
        let response = test_server()
            .process_request(
                r#"{"method":"tools/call","params":{"name":"system_info","arguments":{}}}"#,
            )
            .await;
        assert!(response["content"].is_array());
        assert_ne!(response["isError"], serde_json::json!(true));
    }
}
