//! Voice provider selection — ElevenLabs Agent is primary.

use serde::{Deserialize, Serialize};

/// Supported voice backends. Only ElevenLabs Agent is first-class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceProvider {
    /// ElevenLabs Conversational Agent with client/webhook tools (default).
    #[default]
    ElevenLabsAgent,
    /// Deferred / non-default — not prioritized for tool calling into Impulse.
    #[serde(other)]
    Other,
}

impl VoiceProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ElevenLabsAgent => "elevenlabs_agent",
            Self::Other => "other",
        }
    }

    pub fn is_primary(self) -> bool {
        matches!(self, Self::ElevenLabsAgent)
    }
}

/// Resolve the configured voice provider. Unknown / empty → ElevenLabs Agent.
pub fn default_voice_provider() -> VoiceProvider {
    match std::env::var("IMPULSE_VOICE_PROVIDER") {
        Ok(raw) => {
            let normalized = raw.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "" | "elevenlabs" | "elevenlabs_agent" | "el" | "default" => {
                    VoiceProvider::ElevenLabsAgent
                }
                _ => VoiceProvider::Other,
            }
        }
        Err(_) => VoiceProvider::ElevenLabsAgent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevenlabs_is_default_primary() {
        let p = VoiceProvider::default();
        assert!(p.is_primary());
        assert_eq!(p.as_str(), "elevenlabs_agent");
    }
}
