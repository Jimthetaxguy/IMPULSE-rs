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
//! times its compressed size. [`preflight_pdf_streams`] is what actually
//! bounds memory for that class of stream, run before any page rendering
//! (and so before `BoundedSink`) begins.
//!
//! **Review round 3 architecture (F0/F1/F2/F4), all four findings tied
//! together:**
//!
//! - **F0 (P0):** everything that parses PDF STRUCTURE now happens
//!   exclusively in this process, never the parent. An earlier version
//!   called `pdf_extract::Document::load` in the parent for a cheap page
//!   count; `lopdf::Document::load` eagerly, unconditionally decompresses
//!   every `/Type /ObjStm` object stream as part of loading, with no hook
//!   to intercept or bound it, so a crafted ObjStm blew up the PARENT
//!   itself before any preflight could ever run. That parsing all moved
//!   here: `Document::load`, the trailer-based encryption re-check, the
//!   page-count cap, [`preflight_pdf_streams`], and rendering.
//! - **F2 (memory ceiling):** `Document::load`'s own eager ObjStm inflation
//!   cannot be pre-counted at all -- there is no hook between "lopdf
//!   decides to inflate an object stream" and "lopdf has already inflated
//!   it." [`spawn_memory_watchdog`] is the bound for that specific path: a
//!   background thread polling this process's own peak RSS
//!   ([`current_peak_rss_bytes`]) roughly every 10ms, self-terminating via
//!   `std::process::exit` the instant it crosses
//!   `ion_repl::tool_document::PDF_CHILD_MEMORY_LIMIT_BYTES`. This is also
//!   the ONLY layer that is actually enforced on macOS: the parent's
//!   `RLIMIT_AS` (set via `pre_exec` in `run_pdf_extraction_child`) is
//!   accepted by macOS's `setrlimit` but silently not kernel-enforced there
//!   (confirmed empirically -- a 1 GiB limit let a child reach 4.12 GiB
//!   RSS); it remains real, kernel-enforced defense-in-depth on Linux.
//! - **F1 (P1):** [`preflight_pdf_streams`] previously counted only
//!   `FlateDecode` streams; `lopdf` decodes `LZWDecode` exactly as readily,
//!   and an LZW-filtered bomb reached multi-GB RSS while the preflight
//!   still reported "OK". The preflight now walks each stream's FULL
//!   filter chain (`Stream::filters()`, lopdf's public, decode-ordered
//!   list): an outer `ASCII85Decode`/`ASCIIHexDecode` layer is fully
//!   decoded (cheap, bounded expansion), then the resulting bytes are fed
//!   to a bounded counting decoder for a terminal `FlateDecode` or
//!   `LZWDecode`. Deny-by-default: any filter this preflight cannot
//!   bound-count (`RunLengthDecode`, `DCTDecode`, `JPXDecode`,
//!   `CCITTFaxDecode`, `Crypt`, or a filter chain with anything after a
//!   terminal Flate/LZW stage) is refused by name rather than silently
//!   skipped.
//! - **F4:** the old 64 MiB TOTAL cap refused ordinary documents (a
//!   900-page, 2.87 MB Flate-compressed text PDF inflates to 73.6 MB).
//!   [`MAX_PDF_TOTAL_DECOMPRESSED_BYTES`] is now 512 MiB; the per-stream
//!   cap [`MAX_PDF_STREAM_DECOMPRESSED_BYTES`] stays 64 MiB.

use std::path::Path;

#[cfg(feature = "office-support")]
use anyhow::bail;
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
/// signaled exit. The memory watchdog's self-exit (review round 3, F2)
/// likewise never returns to this function -- it calls
/// `std::process::exit` directly from its own polling thread.
#[cfg(feature = "office-support")]
pub fn run(
    path: &Path,
    max_chars: usize,
    max_pages: usize,
    memory_limit_bytes: Option<u64>,
) -> Result<()> {
    let memory_limit_bytes =
        memory_limit_bytes.unwrap_or(crate::ion_repl::tool_document::PDF_CHILD_MEMORY_LIMIT_BYTES);
    let payload = extract(path, max_chars, max_pages, memory_limit_bytes)?;
    let stdout = std::io::stdout();
    serde_json::to_writer(stdout.lock(), &payload)
        .context("internal-pdf-text: failed to write extraction output")?;
    Ok(())
}

#[cfg(not(feature = "office-support"))]
pub fn run(
    _path: &Path,
    _max_chars: usize,
    _max_pages: usize,
    _memory_limit_bytes: Option<u64>,
) -> Result<()> {
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

// ----------------------------------------------------------------------
// Memory watchdog (review round 3, F2).
// ----------------------------------------------------------------------

/// Reads this process's own peak resident set size via `getrusage`. Used by
/// [`spawn_memory_watchdog`] as the ceiling check -- **not** exact live RSS
/// (that would need `/proc/self/statm` on Linux or a Mach task-info call on
/// macOS), but `ru_maxrss` is monotonic non-decreasing within a process's
/// lifetime, which is exactly what a one-way ceiling check needs: it only
/// ever needs to notice "has this process EVER used more than the limit,"
/// and a poll loop calling this repeatedly will see the true peak the
/// instant it is crossed regardless of any later decrease.
///
/// **Platform unit inconsistency (a well-known libc quirk):** `ru_maxrss`
/// is BYTES on macOS/BSD but KILOBYTES on Linux. This function normalizes
/// both to bytes.
#[cfg(all(unix, feature = "office-support"))]
fn current_peak_rss_bytes() -> u64 {
    // SAFETY: no runtime precondition to validate here -- `RUSAGE_SELF` is
    // a compile-time constant (not attacker-controlled), and `usage` is a
    // `libc::rusage` this stack frame owns and zero-initializes the
    // instant before the call, satisfying `getrusage`'s only real
    // precondition (a valid, appropriately-sized output pointer).
    // `getrusage` only ever writes into `usage` and returns a plain status
    // code; it performs no allocation, cannot panic, and never retains the
    // pointer past this call.
    let usage = unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage
    };
    let raw = usage.ru_maxrss.max(0) as u64;
    #[cfg(target_os = "macos")]
    {
        raw
    }
    #[cfg(not(target_os = "macos"))]
    {
        raw.saturating_mul(1024)
    }
}

/// Spawns a background OS thread that polls this process's own peak RSS via
/// [`current_peak_rss_bytes`] roughly every 10ms and calls
/// `std::process::exit(PDF_MEMORY_CEILING_EXIT_CODE)` the instant it
/// exceeds `limit_bytes` -- the real memory bound on macOS (review round 3,
/// F2), where `RLIMIT_AS` (set by the parent via `pre_exec`) is accepted
/// but not kernel-enforced. On Linux, `RLIMIT_AS` IS enforced, so this
/// watchdog is a second, redundant layer there.
///
/// A background thread rather than an async task deliberately: this
/// process's own rendering work (`pdf_extract`'s recursive content-stream
/// interpreter, `Document::load`'s eager ObjStm decompression, the
/// preflight's decompression loops) all runs synchronously with no
/// `.await` points for a cooperative check to interleave with, so only a
/// genuinely separate OS thread can observe and react to a memory spike
/// while the main thread is blocked inside one of those calls. The
/// returned handle is intentionally never joined: the thread either idles
/// harmlessly for this process's whole lifetime (the common case) or ends
/// the process outright via `exit`, so there is nothing to wait for either
/// way; dropping a `JoinHandle` without joining does not block or panic.
#[cfg(all(unix, feature = "office-support"))]
fn spawn_memory_watchdog(limit_bytes: u64) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        if current_peak_rss_bytes() > limit_bytes {
            std::process::exit(crate::ion_repl::tool_document::PDF_MEMORY_CEILING_EXIT_CODE);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    })
}

/// No portable peak-RSS query is implemented for non-unix targets yet;
/// isolation there still gets crash/timeout containment from the parent,
/// just not this watchdog layer. Returns a handle to a no-op thread so call
/// sites do not need platform-specific branching.
#[cfg(all(not(unix), feature = "office-support"))]
fn spawn_memory_watchdog(_limit_bytes: u64) -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {})
}

// ----------------------------------------------------------------------
// Bounded stream-inflation preflight (review round 2, P2; extended review
// round 3, F1/F3/F4).
// ----------------------------------------------------------------------

/// Largest total inflated bytes every stream object in a PDF may produce
/// combined (review round 3, F4: raised from 64 MiB, which refused
/// ordinary multi-hundred-page documents -- a 900-page, 2.87 MB
/// Flate-compressed text PDF inflates to 73.6 MB, over the old cap).
/// Justified against [`crate::ion_repl::tool_document::MAX_DOCUMENT_BYTES`]
/// (the 10 MiB source-file cap: 512 MiB is a ~51x expansion ceiling on the
/// largest input this tool ever reads) and the separate, independent
/// [`crate::ion_repl::tool_document::MAX_EXTRACTED_CHARS`] budget (16
/// million characters extracted text is capped to, regardless of how much
/// raw decompressed PDF-operator bytes it took to render that much text) --
/// this cap bounds intermediate decompression work, not final output size,
/// which is already bounded independently and more tightly by
/// `BoundedSink`.
#[cfg(feature = "office-support")]
pub(crate) const MAX_PDF_TOTAL_DECOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
/// Per-stream inflation cap: generous for any legitimate single page/Form
/// XObject content stream, far below what a pathological single stream can
/// otherwise reach.
#[cfg(feature = "office-support")]
pub(crate) const MAX_PDF_STREAM_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;

/// Inflates `compressed` through a streaming zlib decoder, discarding each
/// chunk immediately after counting it rather than accumulating output, so
/// peak memory for this call is the decoder's own internal window plus one
/// small read buffer -- never proportional to how much the stream would
/// actually inflate to. Refuses once the running total exceeds `cap`.
#[cfg(feature = "office-support")]
fn inflate_bounded_zlib(compressed: &[u8], cap: u64) -> Result<u64> {
    use std::io::Read as _;
    let mut decoder = flate2::read::ZlibDecoder::new(compressed);
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = decoder
            .read(&mut buf)
            .map_err(|e| anyhow::anyhow!("could not inflate PDF stream (FlateDecode): {e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > cap {
            bail!("PDF stream inflates to more than {cap} bytes via FlateDecode, over the limit");
        }
    }
    Ok(total)
}

/// Inflates `compressed` through `weezl`'s streaming LZW decoder (review
/// round 3, F1), discarding each decoded chunk immediately after counting
/// it into a small fixed output buffer -- peak memory is that buffer plus
/// the decoder's own small internal table, never proportional to how much
/// the stream would actually decode to. Parameters mirror `lopdf`'s own
/// `Stream::decompress_lzw` exactly (PDF/TIFF variant, MSB-first bit order,
/// `EarlyChange` defaulting to true, i.e. an effective minimum code size of
/// 8 passed to `with_tiff_size_switch`), so this decodes the identical byte
/// stream `lopdf` would.
#[cfg(feature = "office-support")]
fn inflate_bounded_lzw(compressed: &[u8], cap: u64) -> Result<u64> {
    let mut decoder = weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8);
    let mut input = compressed;
    let mut out_buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let result = decoder.decode_bytes(input, &mut out_buf);
        total += result.consumed_out as u64;
        if total > cap {
            bail!("PDF stream inflates to more than {cap} bytes via LZWDecode, over the limit");
        }
        input = &input[result.consumed_in..];
        match result.status {
            Ok(weezl::LzwStatus::Ok) => continue,
            Ok(weezl::LzwStatus::Done) => break,
            Ok(weezl::LzwStatus::NoProgress) => {
                if input.is_empty() {
                    // Ran out of input without an explicit end-of-data
                    // code -- treat what decoded so far as the answer
                    // rather than refusing a stream that is merely
                    // missing its EOD marker (lopdf's own decoder is
                    // similarly lenient, logging a warning and returning
                    // what it has rather than failing).
                    break;
                }
                bail!("PDF LZWDecode stream made no decoding progress with input remaining");
            }
            Err(e) => bail!("could not decode PDF LZWDecode stream: {e}"),
        }
    }
    Ok(total)
}

/// Fully decodes an `ASCII85Decode`-filtered outer layer, bounded by `cap`
/// as it goes (the ~4:5 expansion ratio means this is cheap in practice --
/// stream content is itself bounded by the source file's
/// [`crate::ion_repl::tool_document::MAX_DOCUMENT_BYTES`] cap -- but the
/// check is real, not a formality, since a chain could in principle nest
/// several such layers). Reimplemented here rather than calling `lopdf`'s
/// own (private, `Stream::decode_ascii85`) equivalent; the algorithm
/// (groups of 5 base-85 characters decoding to 4 bytes, `z` as shorthand
/// for four zero bytes, an optional trailing `~>` end-of-data marker) is
/// per Adobe's ASCII85 encoding as `lopdf` itself implements it, so this
/// produces byte-for-byte the same output `lopdf` would feed to the next
/// filter in the chain.
#[cfg(feature = "office-support")]
fn decode_ascii85_bounded(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    let trimmed = if input.len() >= 2 && &input[input.len() - 2..] == b"~>" {
        &input[..input.len() - 2]
    } else {
        input
    };
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut count: usize = 0;
    for &ch in trimmed {
        if ch == b'z' {
            if count != 0 {
                bail!("PDF ASCII85Decode stream: 'z' shorthand is not allowed mid-group");
            }
            out.extend_from_slice(&[0, 0, 0, 0]);
            if out.len() as u64 > cap {
                bail!(
                    "PDF stream inflates to more than {cap} bytes via ASCII85Decode, over the \
                     limit"
                );
            }
            continue;
        }
        if ch.is_ascii_whitespace() {
            continue;
        }
        if !(b'!'..=b'u').contains(&ch) {
            break;
        }
        buffer = buffer
            .checked_mul(85)
            .ok_or_else(|| anyhow::anyhow!("PDF ASCII85Decode stream: value overflow"))?;
        buffer += (ch - b'!') as u32;
        count += 1;
        if count == 5 {
            out.extend_from_slice(&buffer.to_be_bytes());
            buffer = 0;
            count = 0;
            if out.len() as u64 > cap {
                bail!(
                    "PDF stream inflates to more than {cap} bytes via ASCII85Decode, over the \
                     limit"
                );
            }
        }
    }
    if count > 0 {
        for _ in count..5 {
            buffer = buffer
                .checked_mul(85)
                .ok_or_else(|| anyhow::anyhow!("PDF ASCII85Decode stream: value overflow"))?;
            buffer += 84;
        }
        let bytes = buffer.to_be_bytes();
        out.extend_from_slice(&bytes[..count - 1]);
        if out.len() as u64 > cap {
            bail!("PDF stream inflates to more than {cap} bytes via ASCII85Decode, over the limit");
        }
    }
    Ok(out)
}

/// Fully decodes an `ASCIIHexDecode`-filtered outer layer, bounded by `cap`
/// as it goes: whitespace is ignored, hex digit pairs decode to one byte
/// each, a trailing lone digit is padded with a low nibble of zero, and a
/// `>` end-of-data marker (if present) terminates the scan.
#[cfg(feature = "office-support")]
fn decode_asciihex_bounded(input: &[u8], cap: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut high_nibble: Option<u8> = None;
    for &b in input {
        if b == b'>' {
            break;
        }
        if b.is_ascii_whitespace() {
            continue;
        }
        let value = (b as char)
            .to_digit(16)
            .ok_or_else(|| anyhow::anyhow!("PDF ASCIIHexDecode stream contains a non-hex byte"))?
            as u8;
        match high_nibble.take() {
            None => high_nibble = Some(value),
            Some(high) => {
                out.push((high << 4) | value);
                if out.len() as u64 > cap {
                    bail!(
                        "PDF stream inflates to more than {cap} bytes via ASCIIHexDecode, over \
                         the limit"
                    );
                }
            }
        }
    }
    if let Some(high) = high_nibble {
        out.push(high << 4);
    }
    Ok(out)
}

/// Walks one stream's full filter chain (review round 3, F1/F3) in decoding
/// order (`Stream::filters()`, `lopdf`'s public accessor -- the same order
/// `Stream::decompressed_content()` itself decodes in) and returns the
/// final decoded byte count, bounded throughout by `per_stream_cap`.
///
/// - No `/Filter` key at all (or a malformed one `lopdf` itself could not
///   parse either): treated as uncompressed content, `Ok(0)` -- there is
///   nothing to bound beyond the source file's own size cap, matching
///   `Stream::decompressed_content()`'s identical "no filter -> raw
///   content" behavior.
/// - `ASCII85Decode`/`ASCIIHexDecode`: fully decoded (bounded, cheap
///   expansion) and fed to the next filter in the chain.
/// - `FlateDecode`/`LZWDecode`: bounded-counted via a streaming decoder.
///   Treated as TERMINAL -- a filter chain with anything after either of
///   these is refused (deny-by-default) rather than silently accepted,
///   since no real-world PDF chain places another filter after the
///   decompression stage and this preflight has no way to bound-count a
///   third stage cheaply.
/// - Anything else (`RunLengthDecode`, `DCTDecode`, `JPXDecode`,
///   `CCITTFaxDecode`, `Crypt`, or an unrecognized name): refused by name.
///   `lopdf` can fail to decode some of these too (`decompressed_content`
///   returns `Unimplemented` for anything outside
///   `Flate`/`LZW`/`ASCII85`), but this preflight refuses them regardless
///   of whether `lopdf` could -- deny-by-default, not "deny only what we
///   know is dangerous."
#[cfg(feature = "office-support")]
fn stream_filter_chain_inflated_bytes(
    stream: &pdf_extract::Stream,
    per_stream_cap: u64,
) -> Result<u64> {
    let filters = match stream.filters() {
        Ok(f) => f,
        Err(_) => return Ok(0),
    };
    if filters.is_empty() {
        return Ok(0);
    }
    let mut current: std::borrow::Cow<'_, [u8]> = std::borrow::Cow::Borrowed(&stream.content);
    for (idx, filter) in filters.iter().enumerate() {
        match *filter {
            b"ASCII85Decode" => {
                current =
                    std::borrow::Cow::Owned(decode_ascii85_bounded(&current, per_stream_cap)?);
            }
            b"ASCIIHexDecode" => {
                current =
                    std::borrow::Cow::Owned(decode_asciihex_bounded(&current, per_stream_cap)?);
            }
            b"FlateDecode" => {
                let total = inflate_bounded_zlib(&current, per_stream_cap)?;
                if idx + 1 != filters.len() {
                    bail!(
                        "PDF stream has a filter after FlateDecode, which this preflight does \
                         not support counting; refusing by default"
                    );
                }
                return Ok(total);
            }
            b"LZWDecode" => {
                let total = inflate_bounded_lzw(&current, per_stream_cap)?;
                if idx + 1 != filters.len() {
                    bail!(
                        "PDF stream has a filter after LZWDecode, which this preflight does not \
                         support counting; refusing by default"
                    );
                }
                return Ok(total);
            }
            other => {
                bail!(
                    "PDF stream uses filter '{}', which this preflight cannot bound-count and \
                     therefore refuses by default",
                    String::from_utf8_lossy(other)
                );
            }
        }
    }
    // The chain ended with only ASCII85/ASCIIHex layers and no terminal
    // Flate/LZW stage (an uncommon but legal case -- a bare ASCII85- or
    // ASCIIHex-encoded content stream): the fully materialized `current` IS
    // the final content, so its length counts directly against the budget.
    Ok(current.len() as u64)
}

/// Inflates every stream object in `doc` once, through
/// [`stream_filter_chain_inflated_bytes`], before any page is rendered --
/// the PDF analogue of `preflight_container`'s zip-container check. See
/// this module's top-level doc comment for the full review round 2/3
/// history.
#[cfg(feature = "office-support")]
pub(crate) fn preflight_pdf_streams(doc: &pdf_extract::Document, raw: &str) -> Result<()> {
    preflight_pdf_streams_with_caps(
        doc,
        raw,
        MAX_PDF_STREAM_DECOMPRESSED_BYTES,
        MAX_PDF_TOTAL_DECOMPRESSED_BYTES,
    )
}

/// [`preflight_pdf_streams`] with explicit per-stream/total caps; the test
/// seam (compressing a fixture large enough to exceed the real caps would
/// be slow for no extra coverage).
#[cfg(feature = "office-support")]
pub(crate) fn preflight_pdf_streams_with_caps(
    doc: &pdf_extract::Document,
    raw: &str,
    per_stream_cap: u64,
    total_cap: u64,
) -> Result<()> {
    let mut total: u64 = 0;
    for object in doc.objects.values() {
        let pdf_extract::Object::Stream(stream) = object else {
            continue;
        };
        let inflated = stream_filter_chain_inflated_bytes(stream, per_stream_cap)
            .map_err(|e| anyhow::anyhow!("{raw} could not be parsed: {e}"))?;
        total = total.saturating_add(inflated);
        if total > total_cap {
            bail!(
                "{raw} PDF stream content inflates to more than {total_cap} bytes combined, \
                 over the limit"
            );
        }
    }
    Ok(())
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
/// recursion bug -- see this module's tests). Authoritative for every PDF
/// safety check (review round 3, F0): the parent
/// (`ion_repl::tool_document::extract_pdf_with_exe_and_timeout`) does
/// nothing but a raw byte scan for `/Encrypt` before spawning this process,
/// so nothing here may be skipped as "already checked upstream."
#[cfg(feature = "office-support")]
fn extract(
    path: &Path,
    max_chars: usize,
    max_pages: usize,
    memory_limit_bytes: u64,
) -> Result<ChildOutput> {
    // Review round 3, F2: started before ANY PDF parsing, including
    // `Document::load` below -- the one attack path (`/Type /ObjStm` eager
    // inflation during load) nothing else in this process can bound.
    let _watchdog = spawn_memory_watchdog(memory_limit_bytes);

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
    // Belt and braces, independent of the byte-level scan above: this
    // process is the sole and authoritative extraction step (review round
    // 3, F0 -- the parent no longer duplicates this check, since it no
    // longer parses PDF structure at all).
    if doc.trailer.get(b"Encrypt").is_ok() {
        anyhow::bail!(
            "{} is an encrypted PDF, which this tool does not support (it never attempts a \
             password, including an empty one); remove the password protection and try again",
            path.display()
        );
    }
    // Bounds memory for every stream NOT routed through an eagerly-inflated
    // `/Type /ObjStm` (review round 2, P2; extended review round 3, F1/F3):
    // BoundedSink bounds output volume and wall clock, not memory -- a
    // FlateDecode or LZWDecode content stream can inflate hundreds of times
    // its compressed size before BoundedSink's first write-time check ever
    // runs. This preflight is what actually bounds memory for that case,
    // discarding inflated bytes as it counts them rather than materializing
    // them. ObjStm's own eager inflation during `Document::load` above is a
    // separate path, bounded only by the watchdog started at the top of
    // this function -- documented honestly, not silently assumed covered.
    preflight_pdf_streams(&doc, &path.display().to_string())?;

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
    use std::path::PathBuf;

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

    // ------------------------------------------------------------------
    // current_peak_rss_bytes / spawn_memory_watchdog (review round 3, F2).
    // ------------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn test_current_peak_rss_bytes_reports_a_plausible_nonzero_value() {
        // This process (the test binary) has necessarily allocated more
        // than a few KB by the time a test body runs; a working
        // getrusage/unit-normalization path should report at least, say,
        // 1 MB, and less than an absurd 1 TB (a sign the unit conversion
        // is wrong -- e.g. treating already-bytes macOS output as
        // kilobytes would inflate this by 1024x).
        let rss = current_peak_rss_bytes();
        assert!(rss > 1_000_000, "implausibly small RSS reported: {rss}");
        assert!(
            rss < 1_000_000_000_000,
            "implausibly large RSS reported (unit bug?): {rss}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_spawn_memory_watchdog_does_not_fire_under_a_generous_limit() {
        // Exercises the real unsafe path (getrusage via the watchdog
        // thread) end to end without actually terminating the test
        // process: a limit far above anything a test binary could
        // plausibly consume in 100ms never trips.
        let _watchdog = spawn_memory_watchdog(u64::MAX);
        std::thread::sleep(std::time::Duration::from_millis(100));
        // If we get here, the watchdog did not call process::exit --
        // the process is still running, which is itself the assertion.
        assert!(current_peak_rss_bytes() > 0);
    }

    // ------------------------------------------------------------------
    // PDF fixture builders (duplicated from `ion_repl::tool_document`'s own
    // `#[cfg(test)]` fixtures -- that module is private, so a sibling
    // module's tests cannot reach into it; this mirrors the same
    // duplication `tests/pdf_extraction_isolation.rs` already accepts for
    // the same reason).
    // ------------------------------------------------------------------

    fn pdf_dict(pairs: &[(&str, pdf_extract::Object)]) -> pdf_extract::Dictionary {
        let mut dict = pdf_extract::Dictionary::new();
        for (key, value) in pairs {
            dict.set(*key, value.clone());
        }
        dict
    }

    fn compress_zlib(data: &[u8]) -> Vec<u8> {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write as _;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn encode_lzw(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
            .into_vec(&mut out)
            .encode_all(data)
            .status
            .unwrap();
        out
    }

    fn encode_ascii85(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in data.chunks(4) {
            let mut buf = [0u8; 4];
            buf[..chunk.len()].copy_from_slice(chunk);
            let v = u32::from_be_bytes(buf);
            if v == 0 && chunk.len() == 4 {
                out.push(b'z');
                continue;
            }
            let mut group = [0u8; 5];
            let mut n = v;
            for slot in group.iter_mut().rev() {
                *slot = 33 + (n % 85) as u8;
                n /= 85;
            }
            out.extend_from_slice(&group[..5 - (4 - chunk.len())]);
        }
        out.extend_from_slice(b"~>");
        out
    }

    /// Builds a one-page PDF whose page content stream declares `filters`
    /// (a `/Filter` name, or a `/Filter` array when more than one) over raw
    /// bytes `content` -- the single low-level building block every PDF
    /// preflight fixture below is built from, mirroring the reviewer's own
    /// Python fixture generator's structure.
    fn write_pdf_with_stream(dir: &tempfile::TempDir, filters: &[&str], content: &[u8]) -> PathBuf {
        use pdf_extract::{Document, Object, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut content_dict = pdf_dict(&[]);
        let filter_obj = match filters {
            [] => None,
            [one] => Some(Object::from(*one)),
            many => Some(Object::Array(
                many.iter().map(|f| Object::from(*f)).collect(),
            )),
        };
        if let Some(filter_obj) = filter_obj {
            content_dict.set("Filter", filter_obj);
        }
        let content_id = doc.add_object(Stream::new(content_dict, content.to_vec()));
        let page_id = doc.add_object(pdf_dict(&[
            ("Type", Object::from("Page")),
            ("Parent", Object::Reference(pages_id)),
            ("Contents", Object::Reference(content_id)),
        ]));
        let pages_dict = pdf_dict(&[
            ("Type", Object::from("Pages")),
            ("Kids", Object::Array(vec![Object::Reference(page_id)])),
            ("Count", Object::Integer(1)),
            (
                "MediaBox",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(612),
                    Object::Integer(792),
                ]),
            ),
        ]);
        doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
        let catalog_id = doc.add_object(pdf_dict(&[
            ("Type", Object::from("Catalog")),
            ("Pages", Object::Reference(pages_id)),
        ]));
        doc.trailer.set("Root", catalog_id);
        let path = dir.path().join("fixture.pdf");
        doc.save(&path).unwrap();
        path
    }

    // ------------------------------------------------------------------
    // preflight_pdf_streams / preflight_pdf_streams_with_caps (moved here
    // from `ion_repl::tool_document` in review round 3, F0, and extended
    // for F1/F3/F4).
    // ------------------------------------------------------------------

    #[test]
    fn test_preflight_pdf_streams_refuses_a_flate_compression_bomb_before_any_page_renders() {
        let dir = tempfile::TempDir::new().unwrap();
        let bomb = compress_zlib(&vec![b'A'; 1_000_000]);
        let path = write_pdf_with_stream(&dir, &["FlateDecode"], &bomb);
        let doc = pdf_extract::Document::load(&path).unwrap();

        let err = preflight_pdf_streams_with_caps(&doc, "bomb.pdf", 10_000, 10_000).unwrap_err();

        assert!(err.to_string().contains("over the limit"), "{err}");
    }

    #[test]
    fn test_preflight_pdf_streams_refuses_an_lzw_bomb_review_round_3_f1() {
        // The F1 finding, at test scale: lopdf decodes LZWDecode streams
        // exactly as readily as FlateDecode, so the preflight must count
        // them too, not just Flate.
        let dir = tempfile::TempDir::new().unwrap();
        let bomb = encode_lzw(&vec![b'A'; 1_000_000]);
        let path = write_pdf_with_stream(&dir, &["LZWDecode"], &bomb);
        let doc = pdf_extract::Document::load(&path).unwrap();

        let err = preflight_pdf_streams_with_caps(&doc, "bomb.pdf", 10_000, 10_000).unwrap_err();

        assert!(err.to_string().contains("over the limit"), "{err}");
    }

    #[test]
    fn test_preflight_pdf_streams_accepts_a_legitimate_flate_document_under_the_cap() {
        let dir = tempfile::TempDir::new().unwrap();
        let content = compress_zlib(b"BT /F1 12 Tf 10 700 Td (hello) Tj ET");
        let path = write_pdf_with_stream(&dir, &["FlateDecode"], &content);
        let doc = pdf_extract::Document::load(&path).unwrap();

        preflight_pdf_streams(&doc, "doc.pdf").unwrap();
    }

    #[test]
    fn test_preflight_pdf_streams_accepts_an_ascii85_plus_flate_legitimate_chain_review_round_3_f3()
    {
        // F3: before this fix, [/ASCII85Decode /FlateDecode] was refused
        // with "corrupt deflate stream" because the ASCII85-encoded outer
        // bytes were fed straight to zlib. The outer layer must be decoded
        // first.
        let dir = tempfile::TempDir::new().unwrap();
        let compressed = compress_zlib(b"BT /F1 12 Tf 10 700 Td (hello) Tj ET");
        let encoded = encode_ascii85(&compressed);
        let path = write_pdf_with_stream(&dir, &["ASCII85Decode", "FlateDecode"], &encoded);
        let doc = pdf_extract::Document::load(&path).unwrap();

        preflight_pdf_streams(&doc, "doc.pdf").unwrap();
    }

    #[test]
    fn test_preflight_pdf_streams_refuses_an_ascii85_plus_flate_bomb() {
        let dir = tempfile::TempDir::new().unwrap();
        let bomb = compress_zlib(&vec![b'A'; 1_000_000]);
        let encoded = encode_ascii85(&bomb);
        let path = write_pdf_with_stream(&dir, &["ASCII85Decode", "FlateDecode"], &encoded);
        let doc = pdf_extract::Document::load(&path).unwrap();

        let err = preflight_pdf_streams_with_caps(&doc, "bomb.pdf", 10_000, 10_000).unwrap_err();

        assert!(err.to_string().contains("over the limit"), "{err}");
    }

    #[test]
    fn test_preflight_pdf_streams_refuses_an_unrecognized_filter_by_name_deny_by_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = write_pdf_with_stream(&dir, &["RunLengthDecode"], b"whatever");
        let doc = pdf_extract::Document::load(&path).unwrap();

        let err = preflight_pdf_streams(&doc, "doc.pdf").unwrap_err();

        assert!(err.to_string().contains("RunLengthDecode"), "{err}");
    }

    #[test]
    fn test_preflight_pdf_streams_accepts_an_uncompressed_stream() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = write_pdf_with_stream(&dir, &[], b"BT /F1 12 Tf 10 700 Td (hi) Tj ET");
        let doc = pdf_extract::Document::load(&path).unwrap();

        preflight_pdf_streams(&doc, "doc.pdf").unwrap();
    }

    // ------------------------------------------------------------------
    // Low-level decoder unit tests.
    // ------------------------------------------------------------------

    #[test]
    fn test_inflate_bounded_zlib_accepts_under_the_cap_and_reports_the_true_size() {
        let payload = vec![b'A'; 1000];
        let compressed = compress_zlib(&payload);

        let size = inflate_bounded_zlib(&compressed, 2000).unwrap();

        assert_eq!(size, 1000);
    }

    #[test]
    fn test_inflate_bounded_zlib_refuses_once_the_running_total_exceeds_the_cap() {
        let payload = vec![b'A'; 1_000_000];
        let compressed = compress_zlib(&payload);
        assert!(
            compressed.len() < 10_000,
            "fixture should compress far smaller than its inflated size: {}",
            compressed.len()
        );

        let err = inflate_bounded_zlib(&compressed, 1000).unwrap_err();

        assert!(err.to_string().contains("over the limit"), "{err}");
    }

    #[test]
    fn test_inflate_bounded_lzw_accepts_under_the_cap_and_reports_the_true_size() {
        let payload = vec![b'A'; 1000];
        let encoded = encode_lzw(&payload);

        let size = inflate_bounded_lzw(&encoded, 2000).unwrap();

        assert_eq!(size, 1000);
    }

    #[test]
    fn test_inflate_bounded_lzw_refuses_once_the_running_total_exceeds_the_cap() {
        let payload = vec![b'A'; 1_000_000];
        let encoded = encode_lzw(&payload);
        assert!(
            encoded.len() < 20_000,
            "fixture should encode far smaller than its decoded size: {}",
            encoded.len()
        );

        let err = inflate_bounded_lzw(&encoded, 1000).unwrap_err();

        assert!(err.to_string().contains("over the limit"), "{err}");
    }

    #[test]
    fn test_decode_ascii85_bounded_round_trips_ordinary_text() {
        let payload = b"Hello, ASCII85 world!";
        let encoded = encode_ascii85(payload);

        let decoded = decode_ascii85_bounded(&encoded, 1000).unwrap();

        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_decode_ascii85_bounded_refuses_once_the_running_total_exceeds_the_cap() {
        let payload = vec![b'A'; 1000];
        let encoded = encode_ascii85(&payload);

        let err = decode_ascii85_bounded(&encoded, 10).unwrap_err();

        assert!(err.to_string().contains("over the limit"), "{err}");
    }

    #[test]
    fn test_decode_asciihex_bounded_round_trips_ordinary_bytes() {
        let payload = b"Hello, hex world!";
        let encoded: Vec<u8> = payload
            .iter()
            .flat_map(|b| format!("{b:02x}").into_bytes())
            .collect();

        let decoded = decode_asciihex_bounded(&encoded, 1000).unwrap();

        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_decode_asciihex_bounded_refuses_once_the_running_total_exceeds_the_cap() {
        let payload = vec![b'A'; 1000];
        let encoded: Vec<u8> = payload
            .iter()
            .flat_map(|b| format!("{b:02x}").into_bytes())
            .collect();

        let err = decode_asciihex_bounded(&encoded, 10).unwrap_err();

        assert!(err.to_string().contains("over the limit"), "{err}");
    }
}
