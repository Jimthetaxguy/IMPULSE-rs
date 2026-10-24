//! Voice provider selection — ElevenLabs Agent is primary.

use serde::{Deserialize, Serialize};

/// Supported voice backends. Only ElevenLabs Agent is first-class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceProvider {
    /// ElevenLabs Conversational Agent with client/webhook tools (default).
    ///
    /// Written as "eleven_labs_agent" (snake_case), but every surface that
    /// names it (`as_str`, `voice status`, `IMPULSE_VOICE_PROVIDER`) says
    /// "elevenlabs_agent". The alias keeps that spelling from falling
    /// through to `Other`.
    #[default]
    #[serde(alias = "elevenlabs_agent")]
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

    #[test]
    fn test_voice_provider_wire_name_and_as_str_spelling_both_parse() {
        // snake_case splits ElevenLabsAgent into "eleven_labs_agent", but
        // `as_str`, `voice status`, the tool-server responses and
        // `IMPULSE_VOICE_PROVIDER` all say "elevenlabs_agent". Without the
        // alias, `#[serde(other)]` turned that spelling into `Other`, which
        // silently demoted the primary provider. The written name is unchanged.
        let rows = crate::test_support::wire_vs_str_api(
            &[VoiceProvider::ElevenLabsAgent, VoiceProvider::Other],
            None,
            |v| v.as_str().to_string(),
        );
        assert_eq!(
            rows,
            crate::test_support::expected_rows(&[
                ("eleven_labs_agent", "elevenlabs_agent", true),
                ("other", "other", true),
            ])
        );
        assert_eq!(
            serde_json::from_str::<VoiceProvider>("\"elevenlabs_agent\"").unwrap(),
            VoiceProvider::ElevenLabsAgent
        );
        assert_eq!(
            serde_json::from_str::<VoiceProvider>("\"eleven_labs_agent\"").unwrap(),
            VoiceProvider::ElevenLabsAgent
        );
        assert_eq!(
            serde_json::from_str::<VoiceProvider>("\"something_else\"").unwrap(),
            VoiceProvider::Other
        );
    }
}
