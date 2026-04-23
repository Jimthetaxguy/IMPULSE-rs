//! Toolkit-neutral theming primitives.
//!
//! Color values are returned as `(u8, u8, u8)` RGB triples so callers in
//! either an egui or a Dioxus renderer can convert without depending on each
//! other's color types.

/// Look up the canonical accent color for a known agent name.
///
/// Returns an `(r, g, b)` triple. Unknown names return the default text
/// color (`#c9d1d9`) so callers always get something to render.
pub fn agent_color_rgb(name: &str) -> (u8, u8, u8) {
    match name {
        "Claude Code" => (0x8b, 0x5c, 0xf6), // purple
        "OpenCode" => (0x3f, 0xb9, 0x50),    // green
        "Codex" => (0xd2, 0x99, 0x22),       // yellow
        "Shell" => (0x58, 0xa6, 0xff),       // blue
        _ => (0xc9, 0xd1, 0xd9),             // default text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_color_rgb_known_agents_match_legacy_palette() {
        // These RGB triples MUST match impulse-term/src/theme.rs:agent_color()
        // — that function will become a thin wrapper around this one.
        assert_eq!(agent_color_rgb("Claude Code"), (0x8b, 0x5c, 0xf6));
        assert_eq!(agent_color_rgb("OpenCode"), (0x3f, 0xb9, 0x50));
        assert_eq!(agent_color_rgb("Codex"), (0xd2, 0x99, 0x22));
        assert_eq!(agent_color_rgb("Shell"), (0x58, 0xa6, 0xff));
    }

    #[test]
    fn test_agent_color_rgb_unknown_agent_returns_default_text() {
        assert_eq!(agent_color_rgb("MysteryAgent"), (0xc9, 0xd1, 0xd9));
        assert_eq!(agent_color_rgb(""), (0xc9, 0xd1, 0xd9));
    }
}
