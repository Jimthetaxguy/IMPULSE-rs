//! Hidden `internal-pdf-text` subcommand: renders one PDF's text layer,
//! bounded, and prints it as JSON to stdout.
//!
//! **Not a public interface.** It exists only to be spawned as an isolated
//! child process by `ion_repl::tool_document::run_pdf_extraction_child`
//! (`#[command(hide = true)]` in `cli.rs` keeps it out of `--help`, and its
//! argument/output shape may change without notice). The isolation itself
//! is the point (review round 1, P0-1/P0-2 on PR #54, Stage 1b-B):
//!
//! - **P0-1:** a crafted PDF whose Form XObject content stream references
//!   itself makes `pdf-extract`'s content-stream interpreter recurse until
//!   the thread's stack is exhausted -- a Rust stack-overflow guard-page
//!   hit, which calls `abort()`, not an unwinding panic.
//!   `tokio::task::spawn_blocking`'s `JoinError` containment cannot catch
//!   an abort, so running this in-process inside `ion` (an ungated tool)
//!   let a hostile PDF kill the whole REPL. Running it here, in a separate
//!   process the parent spawns with `kill_on_drop`/a process-group guard/a
//!   wall-clock timeout, means the crash kills only this process.
//! - **P0-2:** the previous in-process implementation rendered each page
//!   fully into an unbounded `String` before checking it against the
//!   character budget, reaching multiple GB of RSS on a small crafted
//!   file. [`BoundedSink`] fixes this at the source: it refuses a write the
//!   instant the running total would exceed the budget, during rendering,
//!   not after a page completes.
//!
//! **`BoundedSink` bounds OUTPUT VOLUME and WALL CLOCK, not memory in
//! general (review round 2, P2 -- a second, distinct finding from P0-2
//! above, and correcting an earlier doc comment that conflated the two):**
//! `pdf-extract`'s own internal decompression of a stream's content, which
//! happens before `BoundedSink` ever sees a single character, is itself
//! unbounded -- a `FlateDecode` content stream can inflate hundreds of
//! times its compressed size. `extract`'s call to
//! `ion_repl::tool_document::preflight_pdf_streams` is what actually
//! bounds memory, run before any page rendering (and so before
//! `BoundedSink`) begins.

use std::path::Path;

#[cfg(feature = "office-support")]
use anyhow::Context as _;
use anyhow::Result;
#[cfg(feature = "office-support")]
use serde::Serialize;

/// Runs the extraction and writes the JSON result to stdout. Returns `Err`
/// on any failure (encrypted PDF, page-count cap, character budget, a
/// malformed file, or a `pdf-extract` parse error) -- the caller (`main`'s
/// `Result` return, shared by both the `impulse-rs` and `ion` binaries)
/// prints it to stderr and exits non-zero. A crash (stack overflow, OOM,
/// CPU-limit kill) never reaches this function at all: it kills the whole
/// process first, which the PARENT process (not this one) observes as a
/// signaled exit.
#[cfg(feature = "office-support")]
pub fn run(path: &Path, max_chars: usize, max_pages: usize) -> Result<()> {
    let payload = extract(path, max_chars, max_pages)?;
    let stdout = std::io::stdout();
    serde_json::to_writer(stdout.lock(), &payload)
        .context("internal-pdf-text: failed to write extraction output")?;
    Ok(())
}

#[cfg(not(feature = "office-support"))]
pub fn run(_path: &Path, _max_chars: usize, _max_pages: usize) -> Result<()> {
    anyhow::bail!(
        "internal-pdf-text: PDF extraction requires the office-support feature, which this \
         build does not have"
    )
}

#[cfg(feature = "office-support")]
#[derive(Debug, Serialize)]
struct ChildPage {
    page_num: u32,
    text: String,
}

#[cfg(feature = "office-support")]
#[derive(Debug, Serialize)]
struct ChildOutput {
    pages: Vec<ChildPage>,
}

/// A `std::fmt::Write` sink that refuses a write once the RUNNING total
/// across every page processed so far in this process (`count`, seeded
/// from the previous page's final value and never reset between pages)
/// would exceed `max_chars` -- checked per write during rendering
/// (`PlainTextOutput::output_character` calls `write!` once per glyph plus
/// inter-word spacing), not once per page after the whole page has already
/// been built. This is what makes the "check-before-push, same discipline
/// as extract_workbook/extract_word" claim actually true for PDF (review
/// round 1, P0-2): the previous implementation rendered a whole page into
/// an unbounded `String` first and only checked the total afterward, so a
/// single pathological page could grow without bound before the check ever
/// ran. A refused write does not partially apply -- `buf`/`count` are
/// updated together, only after the check passes.
#[cfg(feature = "office-support")]
struct BoundedSink {
    buf: String,
    count: usize,
    max_chars: usize,
}

#[cfg(feature = "office-support")]
impl std::fmt::Write for BoundedSink {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let incoming = s.chars().count();
        if self.count + incoming > self.max_chars {
            return Err(std::fmt::Error);
        }
        self.buf.push_str(s);
        self.count += incoming;
        Ok(())
    }
}

/// `pdf_extract::PlainTextOutput::new<W: ConvertToFmt>` needs the argument
/// TYPE itself to implement `ConvertToFmt`, not merely `fmt::Write` -- the
/// crate only provides that for `&mut String`/`&mut dyn io::Write`/
/// `&mut File`, none of which is `&mut BoundedSink`, so this is the minimal
/// addition needed. `&'a mut BoundedSink: std::fmt::Write` comes for free
/// from std's blanket `impl<W: Write + ?Sized> Write for &mut W`.
#[cfg(feature = "office-support")]
impl<'a> pdf_extract::ConvertToFmt for &'a mut BoundedSink {
    type Writer = &'a mut BoundedSink;
    fn convert(self) -> Self::Writer {
        self
    }
}

/// The actual extraction, independent of the CLI's `run()`/`main()`
/// plumbing so it can be unit-tested directly with fixtures that are known
/// SAFE to call in-process (i.e. never a page that triggers the P0-1
/// recursion bug -- see this module's tests). Re-derives the encryption
/// check and page count itself rather than trusting the parent's own
/// pre-check: this process is the authoritative extraction step, and the
/// parent's pre-check is a cheap optimization to avoid spawning a
/// subprocess for obviously-bad input, not a security boundary this
/// function may skip.
#[cfg(feature = "office-support")]
fn extract(path: &Path, max_chars: usize, max_pages: usize) -> Result<ChildOutput> {
    let bytes =
        std::fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    if crate::ion_repl::tool_document::pdf_declares_encryption(&bytes) {
        anyhow::bail!(
            "{} is an encrypted PDF, which this tool does not support (it never attempts a \
             password, including an empty one); remove the password protection and try again",
            path.display()
        );
    }

    let doc = pdf_extract::Document::load(path)
        .map_err(|e| anyhow::anyhow!("could not parse {}: {e}", path.display()))?;
    // Belt and braces (review round 2), independent of the parent's own
    // identical check in `precheck_pdf`: this process is the authoritative
    // extraction step and re-derives every safety check itself rather than
    // trusting the parent's pre-check, which exists only to avoid spawning
    // a subprocess for obviously-bad input.
    if doc.trailer.get(b"Encrypt").is_ok() {
        anyhow::bail!(
            "{} is an encrypted PDF, which this tool does not support (it never attempts a \
             password, including an empty one); remove the password protection and try again",
            path.display()
        );
    }
    // Bounds memory (review round 2, P2): BoundedSink bounds output volume
    // and wall clock, not memory -- a FlateDecode stream can inflate
    // hundreds of times its compressed size before BoundedSink's first
    // write-time check ever runs. This preflight is what actually bounds
    // memory, discarding inflated bytes as it counts them rather than
    // materializing them.
    crate::ion_repl::tool_document::preflight_pdf_streams(&doc, &path.display().to_string())?;

    let pages = doc.get_pages();
    let page_count = pages.len();
    if page_count > max_pages {
        anyhow::bail!(
            "{} has {page_count} pages, over the {max_pages}-page limit",
            path.display()
        );
    }

    let mut cumulative = 0usize;
    let mut out_pages = Vec::new();
    // Iterate the map's own keys (review round 1 nit) rather than assuming
    // a contiguous 1..=N range: lopdf::Document::get_pages numbers pages
    // sequentially in every version observed, but nothing in its public
    // contract guarantees that, and iterating what it actually returned is
    // strictly more correct for the same cost.
    for &page_num in pages.keys() {
        let mut sink = BoundedSink {
            buf: String::new(),
            count: cumulative,
            max_chars,
        };
        {
            let mut output = pdf_extract::PlainTextOutput::new(&mut sink);
            pdf_extract::output_doc_page(&doc, &mut output, page_num).map_err(|e| match e {
                // Our own sink is the only fmt::Write in this chain, and it
                // only ever errors for one reason -- the budget check
                // above -- so a FormatError here can only mean that.
                pdf_extract::OutputError::FormatError(_) => anyhow::anyhow!(
                    "{} extracted to more than {max_chars} characters, over the limit",
                    path.display()
                ),
                other => anyhow::anyhow!(
                    "could not parse {} (page {page_num}): {other}",
                    path.display()
                ),
            })?;
        }
        cumulative = sink.count;
        let text = sink.buf;
        let trimmed = text.trim_end_matches(['\n', '\r']);
        if trimmed.trim().is_empty() {
            continue;
        }
        out_pages.push(ChildPage {
            page_num,
            text: trimmed.to_string(),
        });
    }

    Ok(ChildOutput { pages: out_pages })
}

#[cfg(all(test, feature = "office-support"))]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // BoundedSink: pure, no PDF/file I/O at all -- exercises the P0-2 fix
    // directly. "Assert the count, not just the error string" (review
    // round 1, item 2): these prove the buffer itself never grows past the
    // cap, including on a single oversized write mirroring the textbomb
    // fixture's one giant `Tj` string.
    // ------------------------------------------------------------------

    #[test]
    fn test_bounded_sink_accepts_writes_up_to_exactly_the_cap() {
        use std::fmt::Write as _;
        let mut sink = BoundedSink {
            buf: String::new(),
            count: 0,
            max_chars: 10,
        };
        sink.write_str("12345").unwrap();
        sink.write_str("67890").unwrap();
        assert_eq!(sink.buf.chars().count(), 10);
        assert_eq!(sink.count, 10);
    }

    #[test]
    fn test_bounded_sink_refuses_the_write_that_would_exceed_the_cap_without_growing_further() {
        use std::fmt::Write as _;
        let mut sink = BoundedSink {
            buf: String::new(),
            count: 0,
            max_chars: 10,
        };
        sink.write_str("1234567890").unwrap();
        let err = sink.write_str("x");
        assert!(err.is_err());
        // A refused write must not partially apply.
        assert_eq!(
            sink.buf.chars().count(),
            10,
            "buffer must not grow past the cap"
        );
        assert_eq!(sink.count, 10, "count must not advance past the cap");
    }

    #[test]
    fn test_bounded_sink_refuses_a_single_oversized_write_without_buffering_any_of_it() {
        // Mirrors the textbomb fixture: one giant Tj string rendered as a
        // single write_str call. The old (pre-isolation) implementation
        // rendered the whole page into an unbounded String and checked
        // only afterward; this sink must reject the write itself, so peak
        // memory for a single oversized write stays O(max_chars), not
        // O(the malicious page's size).
        use std::fmt::Write as _;
        let mut sink = BoundedSink {
            buf: String::new(),
            count: 0,
            max_chars: 100,
        };
        let huge = "A".repeat(10_000_000); // 10 MB in one call
        let err = sink.write_str(&huge);
        assert!(err.is_err());
        assert!(
            sink.buf.is_empty(),
            "an oversized single write must not be buffered at all"
        );
        assert_eq!(sink.count, 0);
    }

    #[test]
    fn test_bounded_sink_seeded_count_carries_the_running_total_across_pages() {
        // The outer extraction loop seeds a fresh BoundedSink's `count`
        // from the PREVIOUS page's final count rather than resetting to 0,
        // so the budget is cumulative across the whole document, matching
        // every other format's check-before-push behavior (extract_word's
        // WordTextBuilder, extract_workbook's cursor).
        use std::fmt::Write as _;
        let mut page1 = BoundedSink {
            buf: String::new(),
            count: 0,
            max_chars: 15,
        };
        page1.write_str("1234567890").unwrap(); // 10 chars
        let mut page2 = BoundedSink {
            buf: String::new(),
            count: page1.count,
            max_chars: 15,
        };
        // 5 more chars fits exactly (10 + 5 = 15 <= 15).
        page2.write_str("abcde").unwrap();
        assert_eq!(page2.count, 15);
        // One more char would exceed the cumulative 15-char budget.
        let err = page2.write_str("f");
        assert!(err.is_err());
        assert_eq!(
            page2.count, 15,
            "the cumulative cap, not a per-page one, applies"
        );
    }
}
