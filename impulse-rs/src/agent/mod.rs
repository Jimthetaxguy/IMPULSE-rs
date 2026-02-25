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

// Chat types - reserved for future use
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Usage,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[async_trait]
#[allow(dead_code)]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_model(&self) -> &str;
    async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse>;
    fn supported_models(&self) -> Vec<&str>;
}

#[allow(dead_code)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub provider: Box<dyn LlmProvider>,
    pub model: String,
    pub system_prompt: Option<String>,
    pub history: Vec<Message>,
}

#[allow(dead_code)]
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
