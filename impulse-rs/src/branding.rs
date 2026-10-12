//! Terminal branding — ASCII art, banners, and engine-state indicators.
//!
//! Provides the block-letter banner, compact tagline, and TUI engine-state
//! art (idle / thinking / success) used across CLI output and the TUI dashboard.

/// ASCII art branding for Impulse CLI.
///
/// Provides the full block-letter banner, compact tagline,
/// and TUI engine-state indicators.
/// Full banner - for status, init, and splash contexts.
pub const BANNER: &str = "\
 ┌─[ IMPULSE ]────────────────────────────────┐
 │ ▧  Feed the impulse to build.             │
 └────────────────────────────────────────────┘";

/// Compact one-liner — for config, verification, and tight contexts.
pub const TAGLINE: &str = "━━━━━✈  IMPULSE · feed the impulse to build";

/// TUI engine-state ASCII art.
pub const ENGINE_IDLE: &str = "\
    ▄▄    
   ████   
  ▗████▖  
  ██  ██  
 ▄██  ██▄ 
██████████
▀██ ██ ██▀
 ▀  ▀▀  ▀ 
          ";

pub const ENGINE_THINKING: &str = "\
    ▄▄    
   ████   
  ▗████▖  
  ██▀▀██  
 ▄██████▄ 
██████████
▀██ ██ ██▀
 ▀  ▀▀  ▀ 
          ";

pub const ENGINE_SUCCESS: &str = "\
    ▄▄    
   ████   
  ▗████▖  
  ██  ██  
 ▄██  ██▄ 
██████████
▀██ ██ ██▀
 ▀ ▄██▄ ▀ 
   ▀██▀   ";

/// Engine state for TUI rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    Idle,
    Thinking,
    Success,
}

/// Returns the engine-state ASCII art for the given state.
pub fn engine_art(state: EngineState) -> &'static str {
    match state {
        EngineState::Idle => ENGINE_IDLE,
        EngineState::Thinking => ENGINE_THINKING,
        EngineState::Success => ENGINE_SUCCESS,
    }
}

/// Prints the full IMPULSE banner to stdout.
pub fn print_banner() {
    println!("{BANNER}");
}

/// Prints a compact section header with the tagline divider.
///
/// Example output:
/// ```text
/// ━━━━━◆  IMPULSE · feed the impulse to build
/// Configuration
/// ```
pub fn print_header(title: &str) {
    println!("{TAGLINE}\n{title}");
}

/// Splits the banner into individual lines for TUI rendering.
pub fn banner_lines() -> Vec<&'static str> {
    BANNER.lines().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_banner_fits_80_columns() {
        for line in BANNER.lines() {
            // Count Unicode grapheme clusters (visual width).
            // Block chars are single-width in most terminals.
            let width: usize = line.chars().count();
            assert!(
                width <= 80,
                "Banner line exceeds 80 columns ({width}): {line:?}"
            );
        }
    }

    #[test]
    fn test_banner_lines_count() {
        let lines = banner_lines();
        assert!(
            lines.len() >= 3,
            "Banner should have at least 3 lines, got {}",
            lines.len()
        );
    }

    #[test]
    fn test_tagline_not_empty() {
        assert!(!TAGLINE.is_empty());
        assert!(TAGLINE.contains("IMPULSE"));
        assert!(TAGLINE.contains("feed the impulse to build"));
        assert!(!TAGLINE.contains("your AI remembers"));
    }

    #[test]
    fn test_engine_states_not_empty() {
        assert!(!engine_art(EngineState::Idle).is_empty());
        assert!(!engine_art(EngineState::Thinking).is_empty());
        assert!(!engine_art(EngineState::Success).is_empty());
    }

    #[test]
    fn test_engine_states_differ() {
        // Each state should produce visually distinct output
        assert_ne!(
            engine_art(EngineState::Idle),
            engine_art(EngineState::Thinking)
        );
        assert_ne!(
            engine_art(EngineState::Idle),
            engine_art(EngineState::Success)
        );
        assert_ne!(
            engine_art(EngineState::Thinking),
            engine_art(EngineState::Success)
        );
    }
}
