//! LLM provider abstraction (Anthropic, OpenAI, Minimax).
//!
//! Defines the [`LlmProvider`] trait and chat interface types ([`Message`],
//! [`ChatRequest`], [`ChatResponse`]). Provider implementations live in
//! [`anthropic`]. Phase 2 API surface — not yet wired to production paths.

pub use crate::error::AgentResult;
pub use async_trait::async_trait;
pub use serde::{Deserialize, Serialize};

// Re-export all providers from consolidated anthropic.rs
pub mod anthropic;
pub use anthropic::{AnthropicProvider, MinimaxProvider, OpenAiProvider};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_model(&self) -> &str;
    async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse>;
    fn supported_models(&self) -> Vec<&str>;
}

pub struct Agent {
    pub id: String,
    pub name: String,
    pub provider: Box<dyn LlmProvider>,
    pub model: String,
    pub system_prompt: Option<String>,
    pub history: Vec<Message>,
}

impl Agent {
    pub fn new(
        id: String,
        name: String,
        provider: Box<dyn LlmProvider>,
        model: Option<String>,
        system_prompt: Option<String>,
    ) -> Self {
        let model = model.unwrap_or_else(|| provider.default_model().to_string());
        Self {
            id,
            name,
            provider,
            model,
            system_prompt,
            history: Vec::new(),
        }
    }

    pub async fn chat(&mut self, user_message: &str) -> AgentResult<String> {
        let mut messages = Vec::new();
        if let Some(ref system) = self.system_prompt {
            messages.push(Message {
                role: Role::System,
                content: system.clone(),
            });
        }
        messages.extend(self.history.clone());
        messages.push(Message {
            role: Role::User,
            content: user_message.to_string(),
        });

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            temperature: 0.7,
            max_tokens: Some(4096),
        };
        let response = self.provider.chat(request).await?;

        self.history.push(Message {
            role: Role::User,
            content: user_message.to_string(),
        });
        self.history.push(Message {
            role: Role::Assistant,
            content: response.content.clone(),
        });

        Ok(response.content)
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant] {
            let json = serde_json::to_string(&role).unwrap();
            let parsed: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn test_message_construction() {
        let msg = Message {
            role: Role::User,
            content: "hello".into(),
        };
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn test_chat_request_serialization() {
        let req = ChatRequest {
            model: "claude-3".into(),
            messages: vec![Message {
                role: Role::User,
                content: "hi".into(),
            }],
            temperature: 0.7,
            max_tokens: Some(4096),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "claude-3");
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.max_tokens, Some(4096));
    }

    #[test]
    fn test_chat_response_serialization() {
        let resp = ChatResponse {
            content: "Hello!".into(),
            model: "claude-3".into(),
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: ChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "Hello!");
        assert_eq!(parsed.usage.input_tokens, 10);
    }

    #[test]
    fn test_role_rename_all_lowercase() {
        let json = serde_json::to_string(&Role::System).unwrap();
        assert_eq!(json, "\"system\"");
        let json = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(json, "\"assistant\"");
    }
}
