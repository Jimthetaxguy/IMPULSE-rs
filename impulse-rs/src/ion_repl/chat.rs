//! Chat-turn wiring for the ion REPL (TUI_SPEC.md T8).
//!
//! TUI_SPEC.md section 2.3 describes free-text turns going through
//! "`llm_backends::ChatSession`" — no literal `ChatSession` type exists in
//! `llm_backends`; [`crate::llm_backends::Agent`] is the substrate that
//! matches the description (`.chat(&mut self, msg) -> AgentResult<String>`
//! with conversation `history` + `.clear_history()`, provider-agnostic via
//! `Box<dyn LlmProvider>`). [`ChatState`] wraps one `Agent` instance, built
//! once per `ReplSession` (`ion_repl::mod::ReplSession::new`) and held
//! mutably across turns so history survives between REPL lines instead of
//! being rebuilt (and lost) on every line.
//!
//! **Missing-key handling:** unlike `handlers::system::handle_chat` (which
//! pre-checks `ANTHROPIC_API_KEY` with an `anyhow::bail!` before ever
//! constructing a provider), `ChatState::from_env` always constructs an
//! `AnthropicProvider` — even with an empty key string — matching the typed
//! `AgentError::MissingApiKey` path used by `ImpulseAgent::new`
//! (`src/agent/mod.rs`). `AnthropicProvider::chat` calls
//! `BaseProvider::check_api_key()` first and returns
//! `Err(AgentError::MissingApiKey { .. })` without ever opening a network
//! connection, so `ChatState::turn` naturally surfaces that typed error for
//! the REPL to render as a one-line notice (`ion_repl::mod::respond`) — no
//! panic, no special-cased startup check needed.

use crate::error::AgentResult;
use crate::llm_backends::{Agent, LlmProvider};

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const SYSTEM_PROMPT: &str =
    "You are ion, an interactive coding-agent assistant running in a terminal REPL.";

/// Holds the `Agent` (provider + model + conversation history) backing free
/// text chat turns in the ion REPL.
pub struct ChatState {
    agent: Agent,
}

impl ChatState {
    /// Builds chat state from `ANTHROPIC_API_KEY`/`CLAUDE_API_KEY` (same
    /// fallback order as `handlers::system::handle_chat`) and `IMPULSE_MODEL`
    /// (override, else `claude-sonnet-4-6`). Never fails and never requires
    /// the key to be present — see the module doc comment for why a missing
    /// key is handled lazily, on first `turn()`, instead of here.
    pub fn from_env() -> Self {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLAUDE_API_KEY"))
            .unwrap_or_default();
        let model = std::env::var("IMPULSE_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let provider: Box<dyn LlmProvider> = Box::new(
            crate::llm_backends::anthropic::AnthropicProvider::new(api_key),
        );
        Self::with_provider(provider, model)
    }

    /// Test/DI seam: build chat state with an arbitrary provider (e.g. a
    /// fake `LlmProvider` from [`test_support`]) instead of the real
    /// Anthropic backend. Also usable by future providers (OpenAI, Minimax).
    pub fn with_provider(provider: Box<dyn LlmProvider>, model: String) -> Self {
        Self {
            agent: Agent::new(
                "ion-repl".to_string(),
                "ion".to_string(),
                provider,
                Some(model),
                Some(SYSTEM_PROMPT.to_string()),
            ),
        }
    }

    /// Sends one chat turn through the underlying `Agent`. Returns the
    /// assistant's reply text, or the `AgentError` the provider failed with
    /// (including `AgentError::MissingApiKey` for an absent key) — never
    /// panics, matching every other `Result`-returning path in this crate.
    pub async fn turn(&mut self, text: &str) -> AgentResult<String> {
        self.agent.chat(text).await
    }

    /// Clears conversation history on the same `Agent` instance (T8 wires
    /// the previously-stubbed `/clear` command to this).
    pub fn clear(&mut self) {
        self.agent.clear_history();
    }

    /// Number of messages (user + assistant) currently held in history.
    /// `#[cfg(test)]`-only accessor used to prove `/clear` actually empties
    /// history rather than merely printing a confirmation string.
    #[cfg(test)]
    pub(crate) fn history_len(&self) -> usize {
        self.agent.history.len()
    }
}

/// Fake `LlmProvider`s for T8 tests. This codebase has no pre-existing test
/// double for `LlmProvider` (`handlers::system::handle_chat`'s tests
/// exercise the real `AnthropicProvider` and accept "may fail with API key
/// or network" rather than mocking — see that module's `mod tests`), so a
/// minimal one is added here, shared by this module's own tests and
/// `ion_repl::mod`'s routing tests, rather than duplicated per test module.
#[cfg(test)]
pub(crate) mod test_support {
    use async_trait::async_trait;

    use crate::error::{AgentError, AgentResult};
    use crate::llm_backends::{ChatRequest, ChatResponse, LlmProvider, Usage};

    /// Echoes the last user message back with a fixed prefix, so tests can
    /// assert the exact text sent by `ChatState::turn` reached the provider.
    pub(crate) struct EchoProvider {
        pub prefix: &'static str,
    }

    #[async_trait]
    impl LlmProvider for EchoProvider {
        fn name(&self) -> &str {
            "echo-fake"
        }
        fn default_model(&self) -> &str {
            "echo-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            let last_user = request
                .messages
                .iter()
                .rev()
                .find(|m| m.role == crate::llm_backends::Role::User)
                .map(|m| m.content.clone())
                .unwrap_or_default();
            Ok(ChatResponse {
                content: format!("{}{}", self.prefix, last_user),
                model: request.model,
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["echo-fake-model"]
        }
    }

    /// Always fails with `AgentError::MissingApiKey`, for tests that want to
    /// exercise the missing-key rendering path without depending on ambient
    /// `ANTHROPIC_API_KEY` env state (which is process-global and shared
    /// across concurrently-running tests).
    pub(crate) struct MissingKeyProvider;

    #[async_trait]
    impl LlmProvider for MissingKeyProvider {
        fn name(&self) -> &str {
            "missing-key-fake"
        }
        fn default_model(&self) -> &str {
            "missing-key-fake-model"
        }
        async fn chat(&self, _request: ChatRequest) -> AgentResult<ChatResponse> {
            Err(AgentError::MissingApiKey {
                provider: "missing-key-fake".to_string(),
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{EchoProvider, MissingKeyProvider};
    use super::*;
    use crate::error::AgentError;

    #[tokio::test]
    async fn test_turn_sends_message_and_returns_provider_reply() {
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".to_string(),
        );
        let reply = chat.turn("hello").await.expect("fake provider succeeds");
        assert_eq!(reply, "echo:hello");
    }

    #[tokio::test]
    async fn test_turn_accumulates_history_across_calls() {
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".to_string(),
        );
        chat.turn("first").await.expect("first turn succeeds");
        assert_eq!(chat.history_len(), 2); // user + assistant
        chat.turn("second").await.expect("second turn succeeds");
        assert_eq!(chat.history_len(), 4);
    }

    #[tokio::test]
    async fn test_clear_resets_history_to_empty() {
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".to_string(),
        );
        chat.turn("hi").await.expect("turn succeeds");
        assert_eq!(chat.history_len(), 2);
        chat.clear();
        assert_eq!(chat.history_len(), 0);
    }

    #[tokio::test]
    async fn test_turn_surfaces_missing_api_key_error_without_panicking() {
        let mut chat = ChatState::with_provider(
            Box::new(MissingKeyProvider),
            "missing-key-fake-model".to_string(),
        );
        let result = chat.turn("hello").await;
        assert!(matches!(result, Err(AgentError::MissingApiKey { .. })));
    }

    #[test]
    fn test_from_env_constructs_without_a_key_present() {
        // Must not panic even when ANTHROPIC_API_KEY/CLAUDE_API_KEY are
        // unset -- construction always succeeds; only `turn()` can fail.
        let _chat = ChatState::from_env();
    }
}
