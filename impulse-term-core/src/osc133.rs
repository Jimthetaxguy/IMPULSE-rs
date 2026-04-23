//! OSC 133 byte-stream parser — extracts shell-integration markers from
//! a PTY byte stream so `BlockStore` can be driven automatically.
//!
//! # The OSC 133 protocol
//!
//! Each marker is an Operating System Command (OSC) sequence with `Ps=133`:
//!
//! | Marker | Bytes | Meaning |
//! |--------|-------|---------|
//! | OSC 133 ; A ST | `ESC ] 133 ; A BEL` (or `ESC \`) | Prompt rendered |
//! | OSC 133 ; B ST | `ESC ] 133 ; B BEL` | Command-input area starts |
//! | OSC 133 ; C ST | `ESC ] 133 ; C BEL` | Command output starts |
//! | OSC 133 ; D [;exit] ST | `ESC ] 133 ; D [ ; <n> ] BEL` | Command done |
//!
//! `ST` = string terminator, either BEL (0x07) or ESC `\` (0x1B 0x5C).
//!
//! Some shell-integration scripts include a payload after the letter
//! (e.g. `OSC 133 ; A ; cl=m ST` for prompt context). The parser
//! tolerates and ignores trailing payload bytes — only the leading
//! letter A/B/C/D and the optional D-exit-code matter for block
//! state transitions.
//!
//! # Streaming-safe
//!
//! `feed(&mut self, &[u8])` accepts arbitrary chunks. The parser holds
//! state across calls so a marker split across two reads (e.g. the BEL
//! terminator arrives in the next chunk) still emits one event.
//!
//! # Non-133 OSCs
//!
//! OSC sequences with other Ps values (e.g. OSC 0 = window title,
//! OSC 8 = hyperlinks, OSC 1337 = iTerm2 protocol) are recognized as
//! OSC sequences and SKIPPED — neither emitted as events nor returned
//! as passthrough bytes through this parser. The vt100 parser running
//! in parallel handles them for display purposes; this parser only
//! emits 133 events.

/// Boundary events the parser emits.
///
/// Map directly to `BlockStore` methods:
/// - `PromptStart`     → `open_prompt()`
/// - `CommandStart`    → `open_command()`
/// - `OutputStart`     → `open_output()`
/// - `CommandEnd`      → `close_with_exit(exit_code)`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Osc133Event {
    PromptStart,
    CommandStart,
    OutputStart,
    CommandEnd { exit_code: Option<i32> },
}

/// Streaming OSC 133 parser. Holds partial-sequence state across
/// `feed` calls.
#[derive(Debug, Clone, Default)]
pub struct Osc133Parser {
    state: State,
    /// Payload accumulator for in-progress OSC sequences. Limited to
    /// `MAX_OSC_PAYLOAD` bytes; over-long sequences are discarded
    /// (they're almost certainly a malformed binary stream, not real
    /// OSC).
    buffer: Vec<u8>,
    /// Whether the OSC currently being read is a 133 (vs. a different
    /// Ps). Determined after the first `;` separator.
    is_133: bool,
}

const MAX_OSC_PAYLOAD: usize = 4096;
const ESC: u8 = 0x1B;
const BEL: u8 = 0x07;
const ST_BACKSLASH: u8 = b'\\';

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum State {
    /// No escape sequence in progress.
    #[default]
    Normal,
    /// Saw `ESC` — waiting for `]` to confirm OSC.
    AfterEsc,
    /// Saw `ESC ]` — accumulating Ps + payload until ST.
    InOsc,
    /// Saw `ESC` while inside an OSC payload — might be the ESC `\`
    /// String Terminator. Next byte determines.
    InOscMaybeSt,
}

impl Osc133Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes from the PTY stream. Returns events for any complete
    /// OSC 133 markers found.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Osc133Event> {
        let mut events = Vec::new();
        for &b in bytes {
            self.feed_byte(b, &mut events);
        }
        events
    }

    fn feed_byte(&mut self, b: u8, events: &mut Vec<Osc133Event>) {
        match self.state {
            State::Normal => {
                if b == ESC {
                    self.state = State::AfterEsc;
                }
                // Else: passthrough byte, no state change.
            }
            State::AfterEsc => match b {
                b']' => {
                    self.state = State::InOsc;
                    self.buffer.clear();
                    self.is_133 = false;
                }
                ESC => {
                    // ESC ESC — stay in AfterEsc waiting for the next byte.
                }
                _ => {
                    // Any other byte after ESC is a different escape
                    // sequence (CSI, SS3, charset, …). Drop back to
                    // Normal — not our concern.
                    self.state = State::Normal;
                }
            },
            State::InOsc => match b {
                BEL => {
                    self.complete_osc(events);
                    self.reset();
                }
                ESC => {
                    self.state = State::InOscMaybeSt;
                }
                _ => {
                    if self.buffer.len() < MAX_OSC_PAYLOAD {
                        self.buffer.push(b);
                        // Detect "133;" prefix as soon as we have it.
                        if !self.is_133 && self.buffer.len() == 4 {
                            self.is_133 = self.buffer.starts_with(b"133;");
                        }
                    } else {
                        // Over-long payload: abandon this OSC entirely.
                        self.reset();
                    }
                }
            },
            State::InOscMaybeSt => {
                if b == ST_BACKSLASH {
                    // ESC \ String Terminator.
                    self.complete_osc(events);
                    self.reset();
                } else {
                    // Lone ESC inside payload (rare but legal). Add the
                    // ESC and the current byte to the buffer.
                    if self.buffer.len() + 2 <= MAX_OSC_PAYLOAD {
                        self.buffer.push(ESC);
                        self.buffer.push(b);
                        self.state = State::InOsc;
                    } else {
                        self.reset();
                    }
                }
            }
        }
    }

    /// Drop all in-progress state (e.g. when a stream resync is needed).
    pub fn reset(&mut self) {
        self.state = State::Normal;
        self.buffer.clear();
        self.is_133 = false;
    }

    fn complete_osc(&mut self, events: &mut Vec<Osc133Event>) {
        if !self.is_133 {
            return; // not an OSC 133, ignore
        }
        // Buffer is now b"133;<letter>[;...]"
        let payload = &self.buffer[4..]; // skip "133;"
        if let Some(event) = parse_133_payload(payload) {
            events.push(event);
        }
    }
}

/// Parse the bytes after `133;` into a single event.
///
/// Examples (the leading `133;` is already consumed):
/// - `b"A"` → PromptStart
/// - `b"A;cl=m"` → PromptStart (extra payload ignored)
/// - `b"D"` → CommandEnd { exit_code: None }
/// - `b"D;0"` → CommandEnd { exit_code: Some(0) }
/// - `b"D;127"` → CommandEnd { exit_code: Some(127) }
fn parse_133_payload(payload: &[u8]) -> Option<Osc133Event> {
    let first = *payload.first()?;
    match first {
        b'A' => Some(Osc133Event::PromptStart),
        b'B' => Some(Osc133Event::CommandStart),
        b'C' => Some(Osc133Event::OutputStart),
        b'D' => {
            let exit_code = if payload.len() > 2 && payload[1] == b';' {
                let tail = &payload[2..];
                // Stop at the next `;` (further payload after exit code).
                let end = tail.iter().position(|&c| c == b';').unwrap_or(tail.len());
                std::str::from_utf8(&tail[..end])
                    .ok()
                    .and_then(|s| s.parse::<i32>().ok())
            } else {
                None
            };
            Some(Osc133Event::CommandEnd { exit_code })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Vec<Osc133Event> {
        let mut p = Osc133Parser::new();
        p.feed(bytes)
    }

    #[test]
    fn test_prompt_start_with_bel() {
        assert_eq!(parse(b"\x1b]133;A\x07"), vec![Osc133Event::PromptStart]);
    }

    #[test]
    fn test_prompt_start_with_st_backslash() {
        assert_eq!(parse(b"\x1b]133;A\x1b\\"), vec![Osc133Event::PromptStart]);
    }

    #[test]
    fn test_command_start() {
        assert_eq!(parse(b"\x1b]133;B\x07"), vec![Osc133Event::CommandStart]);
    }

    #[test]
    fn test_output_start() {
        assert_eq!(parse(b"\x1b]133;C\x07"), vec![Osc133Event::OutputStart]);
    }

    #[test]
    fn test_command_end_with_exit_zero() {
        assert_eq!(
            parse(b"\x1b]133;D;0\x07"),
            vec![Osc133Event::CommandEnd { exit_code: Some(0) }]
        );
    }

    #[test]
    fn test_command_end_with_nonzero_exit() {
        assert_eq!(
            parse(b"\x1b]133;D;127\x07"),
            vec![Osc133Event::CommandEnd {
                exit_code: Some(127)
            }]
        );
    }

    #[test]
    fn test_command_end_without_exit_code() {
        assert_eq!(
            parse(b"\x1b]133;D\x07"),
            vec![Osc133Event::CommandEnd { exit_code: None }]
        );
    }

    #[test]
    fn test_full_lifecycle_in_one_stream() {
        let stream = b"\
            \x1b]133;A\x07\
            \x1b]133;B\x07\
            \x1b]133;C\x07\
            output1\noutput2\n\
            \x1b]133;D;0\x07";
        assert_eq!(
            parse(stream),
            vec![
                Osc133Event::PromptStart,
                Osc133Event::CommandStart,
                Osc133Event::OutputStart,
                Osc133Event::CommandEnd { exit_code: Some(0) },
            ]
        );
    }

    #[test]
    fn test_marker_split_across_two_feeds() {
        let mut p = Osc133Parser::new();
        let first = p.feed(b"\x1b]133;A");
        assert!(first.is_empty(), "no event yet — terminator not arrived");
        let second = p.feed(b"\x07");
        assert_eq!(second, vec![Osc133Event::PromptStart]);
    }

    #[test]
    fn test_marker_split_byte_by_byte() {
        let mut p = Osc133Parser::new();
        let bytes = b"\x1b]133;C\x07";
        let mut all_events = Vec::new();
        for &b in bytes {
            all_events.extend(p.feed(&[b]));
        }
        assert_eq!(all_events, vec![Osc133Event::OutputStart]);
    }

    #[test]
    fn test_passthrough_text_emits_nothing() {
        assert!(parse(b"hello world\nthis is just text\n").is_empty());
    }

    #[test]
    fn test_passthrough_with_marker_inline() {
        let stream = b"prompt$ \x1b]133;A\x07ls\n";
        assert_eq!(parse(stream), vec![Osc133Event::PromptStart]);
    }

    #[test]
    fn test_non_133_osc_is_ignored() {
        // OSC 0 (window title) and OSC 8 (hyperlink) — must NOT emit events.
        let stream = b"\x1b]0;Window Title\x07hello\x1b]8;;https://example.com\x07link\x1b]8;;\x07";
        assert!(parse(stream).is_empty());
    }

    #[test]
    fn test_133_extra_payload_after_letter_is_ignored() {
        // Some shell-integration scripts add `;cl=m` or similar context.
        let stream = b"\x1b]133;A;cl=m\x07";
        assert_eq!(parse(stream), vec![Osc133Event::PromptStart]);

        let stream = b"\x1b]133;D;0;dur=42ms\x07";
        assert_eq!(
            parse(stream),
            vec![Osc133Event::CommandEnd { exit_code: Some(0) }]
        );
    }

    #[test]
    fn test_unknown_133_letter_ignored() {
        // OSC 133;Z is not a defined boundary.
        assert!(parse(b"\x1b]133;Z\x07").is_empty());
    }

    #[test]
    fn test_two_markers_in_one_stream() {
        let stream = b"\x1b]133;A\x07\x1b]133;B\x07";
        assert_eq!(
            parse(stream),
            vec![Osc133Event::PromptStart, Osc133Event::CommandStart]
        );
    }

    #[test]
    fn test_reset_clears_in_progress_state() {
        let mut p = Osc133Parser::new();
        let _ = p.feed(b"\x1b]133;A");
        // Reset mid-stream.
        p.reset();
        // The dangling terminator should no longer emit.
        let evts = p.feed(b"\x07");
        assert!(evts.is_empty());
    }

    #[test]
    fn test_invalid_exit_code_treated_as_none() {
        // "D;abc" — exit code unparseable as i32.
        assert_eq!(
            parse(b"\x1b]133;D;abc\x07"),
            vec![Osc133Event::CommandEnd { exit_code: None }]
        );
    }

    #[test]
    fn test_negative_exit_code_parses() {
        assert_eq!(
            parse(b"\x1b]133;D;-1\x07"),
            vec![Osc133Event::CommandEnd {
                exit_code: Some(-1)
            }]
        );
    }

    #[test]
    fn test_overlong_payload_is_discarded_safely() {
        // Stream a fake OSC with > MAX_OSC_PAYLOAD payload bytes.
        let mut bytes = Vec::with_capacity(MAX_OSC_PAYLOAD + 100);
        bytes.extend_from_slice(b"\x1b]");
        bytes.resize(MAX_OSC_PAYLOAD + 50, b'X');
        bytes.push(BEL);
        // Should not emit anything and should not panic.
        let events = parse(&bytes);
        assert!(events.is_empty());
    }

    #[test]
    fn test_lone_esc_in_payload_does_not_terminate() {
        // ESC followed by a non-`\` byte inside payload is *not* ST;
        // the OSC continues (rare but legal).
        // Simulate "OSC 133;A<ESC>X<BEL>" — ESC X is not ST, so payload
        // becomes "133;A\x1bX". First byte after "133;" is 'A' → PromptStart.
        let stream = b"\x1b]133;A\x1bX\x07";
        assert_eq!(parse(stream), vec![Osc133Event::PromptStart]);
    }

    #[test]
    fn test_mixed_133_and_non_133_osc() {
        // OSC 0 then OSC 133;C — should emit only the OutputStart.
        let stream = b"\x1b]0;Title\x07\x1b]133;C\x07";
        assert_eq!(parse(stream), vec![Osc133Event::OutputStart]);
    }
}
