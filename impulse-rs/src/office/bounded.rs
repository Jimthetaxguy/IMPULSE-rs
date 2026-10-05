//! Bounded readers for `xlsx`, `docx` and `csv` files, shared by the
//! `office` entry points (the `office` CLI, the `excel_read`, `word_read`
//! and `document_parse` tools, and the office context provider) and Ion's
//! `document_read`, which moved them here from `ion_repl::tool_document`.
//!
//! Documents come from third parties, so a hostile file must not be able to
//! exhaust memory. Bounds, in the order [`read_document`] applies them:
//!
//! - legacy `xls` is refused: its binary format has no streaming reader, and
//!   calamine builds every sheet's dense grid as it opens the file;
//! - the source file must be a regular file of at most
//!   [`MAX_DOCUMENT_BYTES`];
//! - an `xlsx`/`docx` zip container is inflated once, entry by entry,
//!   through [`MAX_DECOMPRESSED_BYTES`] before any parser runs
//!   ([`preflight_container`]), so a forged central directory cannot hide a
//!   decompression bomb;
//! - workbooks are streamed cell by cell into text under
//!   [`MAX_EXTRACTED_CHARS`] and [`MAX_CELLS`] ([`extract_workbook`]); the
//!   dense-grid parser is never used, so two cells at opposite corners of a
//!   sheet cost two cells and a gap marker rather than billions of empty
//!   cells;
//! - Word documents are streamed event by event from `word/document.xml`
//!   ([`extract_word`]), so the docx object tree is never built;
//! - `csv` is read whole, which the file cap bounds, and must be UTF-8
//!   ([`extract_csv`]).
//!
//! These bound the parsers' inputs and outputs; they are not an OS sandbox.
//! The extractors are synchronous, so async callers run them on the
//! blocking pool.
//!
//! Every function that can fail takes a `label`, the subject its error
//! messages begin with: Ion passes `document_read: 'q3.xlsx'`, and the
//! `office` entry points pass the quoted path ([`path_label`]).

use std::path::Path;

use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};

/// Largest source file read.
pub const MAX_DOCUMENT_BYTES: u64 = 10 * 1024 * 1024;
/// Largest total size the entries of an `xlsx`/`docx` zip container may
/// inflate to. Every entry is inflated once through this cap before a parser
/// runs.
pub const MAX_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
/// Largest text extracted from one document, in characters.
pub const MAX_EXTRACTED_CHARS: usize = 16_000_000;
/// Largest number of non-empty cells streamed from one workbook.
pub const MAX_CELLS: u64 = 2_000_000;
/// Empty columns inside a row rendered as bare tabs; wider gaps become a
/// marker so a cell far to the right cannot inflate the text.
pub const EMPTY_COLUMNS_INLINE: u32 = 8;

const SHEET_HEADER_PREFIX: &str = "=== Sheet: ";
const SHEET_HEADER_SUFFIX: &str = " ===";

/// One addressable part of a document: a sheet, a CSV body, or a run of
/// paragraphs. `offset` and `chars` are in the same character coordinates
/// as the paged whole-document text, so `offset` can be passed back to jump
/// to the section. `offset` is absent only when a parser's layout could not
/// be matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSection {
    pub index: usize,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    pub chars: usize,
}

/// One non-empty worksheet as this module rendered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetBody {
    pub name: String,
    /// Position in the workbook, counting empty sheets.
    pub index: usize,
    pub body: String,
}

/// Everything a bounded read produced from one file, in the character
/// coordinates of `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocument {
    pub format: String,
    pub document_type: String,
    pub size_bytes: u64,
    /// The whole-document text; Ion's `document_read` pages through it.
    pub text: String,
    pub sections: Vec<DocumentSection>,
    /// Worksheet bodies in workbook order (`xlsx` only; empty sheets are
    /// omitted).
    pub sheets: Vec<SheetBody>,
}

/// How much a single extraction may produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractBudget {
    pub max_chars: usize,
    pub max_cells: u64,
}

impl ExtractBudget {
    pub const DEFAULT: Self = Self {
        max_chars: MAX_EXTRACTED_CHARS,
        max_cells: MAX_CELLS,
    };
}

/// Inflates every entry of an `xlsx`/`docx` zip container once through
/// [`MAX_DECOMPRESSED_BYTES`] before any parser sees it, so neither a
/// forged central directory nor a highly compressible entry can balloon
/// during parsing. Other formats (`csv`) are not containers and pass.
pub fn preflight_container(path: &Path, label: &str) -> Result<()> {
    preflight_container_with_limit(path, label, MAX_DECOMPRESSED_BYTES)
}

/// [`preflight_container`] with an explicit inflation cap; the test seam.
pub fn preflight_container_with_limit(
    path: &Path,
    label: &str,
    max_uncompressed: u64,
) -> Result<()> {
    use std::io::Read as _;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(ext.as_str(), "xlsx" | "docx") {
        return Ok(());
    }
    let file = std::fs::File::open(path).with_context(|| format!("{label} could not be opened"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        anyhow::anyhow!("{label} could not be parsed: not a valid {ext} container ({e})")
    })?;
    let mut inflated_total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| {
            anyhow::anyhow!("{label} could not be parsed: unreadable {ext} entry ({e})")
        })?;
        let remaining = max_uncompressed.saturating_sub(inflated_total);
        let name = entry.name().to_string();
        let inflated = std::io::copy(
            &mut (&mut entry).take(remaining.saturating_add(1)),
            &mut std::io::sink(),
        )
        .with_context(|| format!("{label} entry '{name}' failed to inflate"))?;
        if inflated > remaining {
            bail!(
                "{label} inflates to more than {max_uncompressed} bytes of \
                 uncompressed content, over the limit"
            );
        }
        inflated_total += inflated;
    }
    Ok(())
}

/// Refuses extracted text above `max_chars`.
pub fn check_extracted_size(text: &str, label: &str, max_chars: usize) -> Result<()> {
    let chars = text.chars().count();
    if chars > max_chars {
        bail!(
            "{label} extracted to {chars} characters, over the {max_chars} \
             character limit"
        );
    }
    Ok(())
}

/// The subject the `office` entry points give error messages: the path,
/// quoted.
pub fn path_label(path: &Path) -> String {
    format!("'{}'", path.display())
}

/// Returns `path`'s extension, lowercased, when it is one of `accepted`, and
/// refuses it otherwise. Legacy `xls` gets its own explanation.
pub fn check_extension(path: &Path, label: &str, accepted: &[&str]) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if accepted.contains(&ext.as_str()) {
        return Ok(ext);
    }
    if ext == "xls" {
        bail!(
            "{label} is a legacy .xls workbook, which is not read because its binary format \
             cannot be bounded before parsing; convert it to .xlsx"
        );
    }
    Err(anyhow::anyhow!(
        "{label} has unsupported extension '{ext}' (supported: {})",
        accepted.join(", ")
    ))
}

/// Refuses anything but a regular file of at most `max_bytes`, and returns
/// its size. The check is by path, before the file is opened, as in Ion's
/// `resolve_document_path`.
pub fn check_source_file(path: &Path, label: &str, max_bytes: u64) -> Result<u64> {
    let metadata = std::fs::metadata(path).with_context(|| format!("{label} could not be read"))?;
    if !metadata.is_file() {
        bail!("{label} is not a regular file");
    }
    if metadata.len() > max_bytes {
        bail!(
            "{label} is {} bytes, over the {max_bytes}-byte limit",
            metadata.len()
        );
    }
    Ok(metadata.len())
}

/// Reads a `csv` file whole as UTF-8 text with one `csv` section. Its memory
/// is bounded by [`MAX_DOCUMENT_BYTES`]: the read stops one byte past the
/// cap, so a file that grew after [`check_source_file`] is refused rather
/// than read whole.
pub fn extract_csv(path: &Path, label: &str, budget: ExtractBudget) -> Result<ParsedDocument> {
    use std::io::Read as _;

    let file = std::fs::File::open(path).with_context(|| format!("{label} could not be opened"))?;
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("{label} could not be read"))?;
    let size_bytes = bytes.len() as u64;
    if size_bytes > MAX_DOCUMENT_BYTES {
        bail!("{label} is more than {MAX_DOCUMENT_BYTES} bytes, over the limit");
    }
    let text = String::from_utf8(bytes).map_err(|e| {
        anyhow::anyhow!(
            "{label} is not valid UTF-8 (first invalid byte at offset {})",
            e.utf8_error().valid_up_to()
        )
    })?;
    check_extracted_size(&text, label, budget.max_chars)?;
    let chars = text.chars().count();
    let sections = if chars == 0 {
        Vec::new()
    } else {
        vec![DocumentSection {
            index: 0,
            kind: "csv".to_string(),
            name: None,
            offset: Some(0),
            chars,
        }]
    };
    Ok(ParsedDocument {
        format: "csv".to_string(),
        document_type: "excel".to_string(),
        size_bytes,
        text,
        sections,
        sheets: Vec::new(),
    })
}

/// Reads an `xlsx`, `docx` or `csv` file under every bound in this module,
/// in order: the extension, the source file, the container preflight, then
/// the streaming extractor. The `office` entry points call this; Ion's
/// `document_read` applies the same steps itself, around its own path
/// sandbox.
pub fn read_document(path: &Path, label: &str, budget: ExtractBudget) -> Result<ParsedDocument> {
    contain_panics(label, || {
        let ext = check_extension(path, label, &["xlsx", "docx", "csv"])?;
        check_source_file(path, label, MAX_DOCUMENT_BYTES)?;
        preflight_container(path, label)?;
        match ext.as_str() {
            "xlsx" => extract_workbook(path, label, budget),
            "docx" => extract_word(path, label, budget),
            _ => extract_csv(path, label, budget),
        }
    })
}

/// Runs `read`, turning a panic inside a parser into an error for this one
/// document. With overflow checks on, as in debug and test builds, calamine
/// panics on some malformed workbooks: an inverted `<dimension
/// ref="B2:A1">`, or a cell reference whose row or column overflows `u32`.
/// The office CLI and context provider call the readers synchronously,
/// where that panic would unwind the caller; async callers already contain
/// it through `spawn_blocking`. The default panic hook still prints the
/// panic message.
pub fn contain_panics<T>(label: &str, read: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).unwrap_or_else(|payload| {
        let detail = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".to_string());
        Err(anyhow::anyhow!(
            "{label} could not be parsed: the parser panicked ({detail})"
        ))
    })
}

/// Streams a workbook through calamine's cell reader into this module's own
/// text: `=== Sheet: name ===`, the sheet body, and a blank line per
/// non-empty sheet, with sections and offsets computed as the text is built.
/// The dense-grid parser is never used, so a far-off cell costs only its
/// gap marker and a shared string costs only the cells that render it.
pub fn extract_workbook(path: &Path, label: &str, budget: ExtractBudget) -> Result<ParsedDocument> {
    use calamine::{open_workbook, DataRef, Reader, Xlsx};

    let mut workbook: Xlsx<_> =
        open_workbook(path).map_err(|e| anyhow::anyhow!("{label} could not be parsed: {e}"))?;
    let names = workbook.sheet_names().to_vec();
    let mut text = String::new();
    let mut cursor = 0usize;
    let mut sections = Vec::new();
    let mut sheets = Vec::new();
    let mut cells_total: u64 = 0;

    for (index, name) in names.iter().enumerate() {
        let mut reader = match workbook.worksheet_cells_reader(name) {
            Ok(reader) => reader,
            // Chart and dialog sheets are listed alongside worksheets but hold
            // no cells; treat them as empty sheets, keeping their workbook
            // position, the way calamine's own range reader does.
            Err(calamine::XlsxError::NotAWorksheet(_)) => continue,
            Err(e) => bail!("{label} could not be parsed: sheet '{name}': {e}"),
        };
        // A non-empty sheet adds its header and a closing blank line as
        // well as its body. All three count against the budget, so a long
        // sheet name cannot carry the text past it.
        let header = format!("{SHEET_HEADER_PREFIX}{name}{SHEET_HEADER_SUFFIX}\n");
        let header_chars = header.chars().count();
        let mut body = SheetBodyBuilder::default();
        while let Some(cell) = reader
            .next_cell()
            .map_err(|e| anyhow::anyhow!("{label} could not be parsed: sheet '{name}': {e}"))?
        {
            let value = match cell.get_value() {
                DataRef::Empty => continue,
                DataRef::Int(i) => i.to_string(),
                DataRef::Float(f) => f.to_string(),
                DataRef::String(s) => s.clone(),
                DataRef::SharedString(s) => (*s).to_string(),
                DataRef::Bool(b) => b.to_string(),
                DataRef::DateTime(dt) => dt.to_string(),
                DataRef::DateTimeIso(s) | DataRef::DurationIso(s) => s.clone(),
                DataRef::Error(e) => e.to_string(),
            };
            cells_total += 1;
            if cells_total > budget.max_cells {
                bail!(
                    "{label} has more than {} non-empty cells, over the limit",
                    budget.max_cells
                );
            }
            let (row, col) = cell.get_position();
            body.push(row, col, &value);
            if cursor + header_chars + body.chars + 2 > budget.max_chars {
                bail!(
                    "{label} extracted to more than {} characters, over the limit",
                    budget.max_chars
                );
            }
        }
        let (body_text, body_chars) = body.finish();
        if body_text.is_empty() {
            continue;
        }
        cursor += header_chars;
        sections.push(DocumentSection {
            index,
            kind: "sheet".to_string(),
            name: Some(name.clone()),
            offset: Some(cursor),
            chars: body_chars,
        });
        text.push_str(&header);
        text.push_str(&body_text);
        text.push_str("\n\n");
        cursor += body_chars + 2;
        sheets.push(SheetBody {
            name: name.clone(),
            index,
            body: body_text,
        });
    }

    let size_bytes = std::fs::metadata(path)
        .with_context(|| format!("{label} could not be read"))?
        .len();
    Ok(ParsedDocument {
        format: "xlsx".to_string(),
        document_type: "excel".to_string(),
        size_bytes,
        text,
        sections,
        sheets,
    })
}

/// Paragraphs per Word section in the outline.
const WORD_PARAGRAPHS_PER_SECTION: usize = 10;
/// Most outline sections one Word document may contribute. The section
/// table travels in the payload, so a document made of millions of
/// one-character paragraphs would otherwise grow it without bound. Past the
/// cap no new section starts and the last one absorbs the remaining text,
/// so every offset that is reported stays truthful.
pub const MAX_WORD_SECTIONS: usize = 4_096;

/// Element subtrees inside `word/document.xml` whose text is not part of
/// what a reader sees, and which are therefore never extracted:
///
/// - `w:del` and `w:moveFrom` -- a tracked deletion and the source half of
///   a tracked move. Deleted runs carry `w:delText` rather than `w:t` and
///   so would be dropped anyway; moved-from runs carry ordinary `w:t` and
///   would otherwise duplicate their `w:moveTo` counterpart.
/// - `w:instrText` and `w:delInstrText` -- field instruction codes such as
///   `MERGEFIELD Name`, never the field result the reader sees, which is a
///   sibling `w:t`.
/// - `mc:Fallback` -- the legacy half of an `mc:AlternateContent` pair,
///   whose text repeats the `mc:Choice` that is used instead.
///
/// Matching is on the local name, so the namespace prefix a writer chose
/// does not matter. Two consequences of that are deliberate: a simple field
/// (`w:fldSimple`) keeps its cached result, because its instruction lives in
/// a `w:instr` **attribute** and attributes are never read; and text a writer
/// put in a shape or text box (DrawingML `<a:t>`) or an equation (OMML
/// `<m:t>`) is extracted like any other `t`, because a reader sees it too.
fn is_skipped_word_subtree(local_name: &[u8]) -> bool {
    matches!(
        local_name,
        b"del" | b"moveFrom" | b"instrText" | b"delInstrText" | b"Fallback"
    )
}

/// Accumulates extracted Word lines into the whole-document text and its
/// outline, enforcing the character budget as each line lands. It is kept
/// separate from the XML walk so its boundaries -- an empty document, a
/// budget hit exactly, section grouping, and the section cap -- are
/// testable without building a document.
#[derive(Debug)]
pub struct WordTextBuilder {
    text: String,
    cursor: usize,
    sections: Vec<DocumentSection>,
    paragraphs_in_section: usize,
    max_chars: usize,
}

impl WordTextBuilder {
    pub fn new(max_chars: usize) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            sections: Vec::new(),
            paragraphs_in_section: 0,
            max_chars,
        }
    }

    /// Characters already committed to the text.
    pub fn chars(&self) -> usize {
        self.cursor
    }

    /// Refuses `pending` further characters before a buffer grows to hold
    /// them, so the budget bounds every buffer in flight and not only the
    /// committed text.
    pub fn check_pending(&self, pending: usize, label: &str) -> Result<()> {
        if self.cursor + pending > self.max_chars {
            bail!(
                "{label} extracted to more than {} characters, over the limit",
                self.max_chars
            );
        }
        Ok(())
    }

    /// Commits one non-empty line (a paragraph, or a whole table row) and
    /// its trailing newline. Empty lines are dropped, so a document of
    /// blank paragraphs produces no text, no sections, and no growth.
    ///
    /// Any `\n` or `\r` still inside `line` becomes a space, one character
    /// for one, so the invariant every consumer relies on holds absolutely:
    /// one output line is exactly one paragraph or one table row, and the
    /// only newlines in the text are the ones this method writes. `window`
    /// snapping and section spans are exact because of it. The caller
    /// already normalizes document text, so this is the backstop, not the
    /// only guard.
    pub fn push_line(&mut self, line: &str, label: &str) -> Result<()> {
        if line.trim().is_empty() {
            return Ok(());
        }
        let line = if line.contains(['\n', '\r']) {
            std::borrow::Cow::Owned(line.replace(['\n', '\r'], " "))
        } else {
            std::borrow::Cow::Borrowed(line)
        };
        let line = line.as_ref();
        let chars = line.chars().count() + 1;
        self.check_pending(chars, label)?;
        if self.paragraphs_in_section == 0 && self.sections.len() < MAX_WORD_SECTIONS {
            self.sections.push(DocumentSection {
                index: self.sections.len(),
                kind: "paragraph".to_string(),
                name: None,
                offset: Some(self.cursor),
                chars: 0,
            });
        }
        self.text.push_str(line);
        self.text.push('\n');
        self.cursor += chars;
        if let Some(section) = self.sections.last_mut() {
            section.chars += chars;
        }
        self.paragraphs_in_section = (self.paragraphs_in_section + 1) % WORD_PARAGRAPHS_PER_SECTION;
        Ok(())
    }

    /// The whole-document text and its section table.
    pub fn finish(self) -> (String, Vec<DocumentSection>) {
        (self.text, self.sections)
    }
}

/// Copies document text into the paragraph buffer, replacing the control
/// characters that would otherwise let text forge structure: `\n` and `\r`
/// always, because one output line is one paragraph or one row, and `\t`
/// as well inside a table row, where a tab is the column separator. Every
/// replacement is one character for one, so the caller's character count
/// stays exact. Structural breaks are written only by the walk itself, from
/// `w:tab` outside a table and from a row or paragraph ending.
fn push_word_text(paragraph: &mut String, text: &str, in_row: bool) {
    for ch in text.chars() {
        paragraph.push(match ch {
            '\n' | '\r' => ' ',
            '\t' if in_row => ' ',
            other => other,
        });
    }
}

/// Streams a Word document's `word/document.xml` through quick-xml into
/// this module's own text under the character budget. The docx object
/// tree, which is many times the size of the XML it is built from, is never
/// materialized, so a small file that inflates to millions of empty
/// paragraphs costs only the time to walk past them.
///
/// Layout: one line per non-empty paragraph, and one line per table row
/// with its cells tab-separated, matching how workbook rows are rendered;
/// paragraphs inside one cell are joined with spaces. That mapping is
/// absolute -- an output line is always exactly one paragraph or one row --
/// so `w:br`/`w:cr` become spaces and document text can never contribute a
/// `\n`, `\r`, or (inside a row) a `\t` of its own. Outside a table `w:tab`
/// stays a tab, which cannot break a line. Nested tables flatten into the
/// row that contains them, and a cell's own text survives a table nested
/// inside it.
///
/// Every buffer that holds text on its way to the output -- the paragraph,
/// the table cell, the table row -- is checked against the budget *before*
/// it grows, so no long-lived buffer can overshoot on one event. Peak
/// memory is not the budget alone: quick-xml's event buffer holds one event
/// and its open-element stack scales with nesting depth, both bounded by
/// the part rather than by the budget, so the working bound is roughly
/// twice [`MAX_DECOMPRESSED_BYTES`]. `word/document.xml` is held to that cap
/// here even when [`preflight_container`] has not run, and a part that
/// exceeds it is refused rather than silently truncated. quick-xml resolves
/// only the five predefined XML entities, so no declared or nested entity
/// expands here.
///
/// The part is located the way calamine locates workbook parts, comparing
/// names ASCII-case-insensitively, because OPC part names are not
/// case-sensitive.
///
/// Only `word/document.xml` is read. Headers, footers, footnotes, endnotes,
/// and comments live in sibling parts and are deliberately out of scope.
pub fn extract_word(path: &Path, label: &str, budget: ExtractBudget) -> Result<ParsedDocument> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    use std::io::Read as _;

    const DOCUMENT_PART: &str = "word/document.xml";

    let file = std::fs::File::open(path).with_context(|| format!("{label} could not be opened"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        anyhow::anyhow!("{label} could not be parsed: not a valid docx container ({e})")
    })?;
    // OPC part names compare case-insensitively, and calamine resolves
    // workbook parts the same way; a writer that emits `word/Document.xml`
    // produces a file every reader opens, so this one opens it too.
    let part = archive
        .file_names()
        .find(|name| name.eq_ignore_ascii_case(DOCUMENT_PART))
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("{label} could not be parsed: no {DOCUMENT_PART}"))?;
    let entry = archive
        .by_name(&part)
        .map_err(|e| anyhow::anyhow!("{label} could not be parsed: no {DOCUMENT_PART} ({e})"))?;
    // One byte past the cap, so that running the limit to zero proves the
    // part is over it: this path fails closed rather than parsing whatever
    // prefix fits when `preflight_container` has not already run.
    let mut reader = Reader::from_reader(std::io::BufReader::new(
        entry.take(MAX_DECOMPRESSED_BYTES + 1),
    ));
    let mut buf = Vec::new();
    let mut out = WordTextBuilder::new(budget.max_chars);

    // Text in flight: the current paragraph, the table cell the finished
    // paragraphs of a cell are joined into, and the row those cells are
    // joined into. Their character counts are tracked as they grow so the
    // budget applies before a buffer balloons, not only when a line lands.
    let mut paragraph = String::new();
    let mut cell = String::new();
    let mut row = String::new();
    let mut paragraph_chars = 0usize;
    let mut cell_chars = 0usize;
    let mut row_chars = 0usize;

    let mut in_text_run = false;
    // Depth inside a subtree whose text is not extracted; 0 means visible.
    let mut skip_depth = 0usize;
    // Depth of nested `w:tr`; 0 means not inside a table row.
    let mut row_depth = 0usize;
    // Depth of nested `w:tc`. Only the outermost cell of a row becomes a
    // column, so a table nested inside a cell merges into that cell instead
    // of erasing the text beside it and adding a column of its own.
    let mut cell_depth = 0usize;
    let mut cells_in_row = 0usize;

    // A read error is held rather than returned, so that a part cut off at
    // the cap is reported as too large -- which it is -- and not as a
    // malformed document, which it may well not be.
    let mut malformed: Option<anyhow::Error> = None;
    loop {
        let event = match reader.read_event_into(&mut buf) {
            Ok(event) => event,
            Err(e) => {
                malformed = Some(anyhow::anyhow!(
                    "{label} could not be parsed: {DOCUMENT_PART} is malformed \
                     ({e})"
                ));
                break;
            }
        };
        match event {
            Event::Start(ref e) => {
                if skip_depth > 0 {
                    skip_depth += 1;
                } else if is_skipped_word_subtree(e.local_name().as_ref()) {
                    skip_depth = 1;
                } else {
                    match e.local_name().as_ref() {
                        b"p" => {
                            paragraph.clear();
                            paragraph_chars = 0;
                        }
                        b"t" => in_text_run = true,
                        b"tr" => {
                            if row_depth == 0 {
                                row.clear();
                                cell.clear();
                                row_chars = 0;
                                cell_chars = 0;
                                cells_in_row = 0;
                                cell_depth = 0;
                            }
                            row_depth += 1;
                        }
                        b"tc" => {
                            if cell_depth == 0 {
                                cell.clear();
                                cell_chars = 0;
                            }
                            cell_depth += 1;
                        }
                        _ => {}
                    }
                }
            }
            Event::Empty(ref e) if skip_depth == 0 => match e.local_name().as_ref() {
                // A tab outside a table is ordinary text; inside one the tab
                // is the column separator, so it becomes a space.
                b"tab" => {
                    out.check_pending(paragraph_chars + 1 + cell_chars + row_chars + 1, label)?;
                    paragraph.push(if row_depth > 0 { ' ' } else { '\t' });
                    paragraph_chars += 1;
                }
                // A line break inside a paragraph becomes a space: one
                // output line is one paragraph or one row, always.
                b"br" | b"cr" => {
                    out.check_pending(paragraph_chars + 1 + cell_chars + row_chars + 1, label)?;
                    paragraph.push(' ');
                    paragraph_chars += 1;
                }
                b"p" => {
                    paragraph.clear();
                    paragraph_chars = 0;
                }
                _ => {}
            },
            Event::Text(ref t) if skip_depth == 0 && in_text_run => {
                let unescaped = t.unescape().map_err(|e| {
                    anyhow::anyhow!("{label} could not be parsed: bad text escape ({e})")
                })?;
                let chars = unescaped.chars().count();
                out.check_pending(paragraph_chars + chars + cell_chars + row_chars + 1, label)?;
                push_word_text(&mut paragraph, &unescaped, row_depth > 0);
                paragraph_chars += chars;
            }
            Event::CData(ref c) if skip_depth == 0 && in_text_run => {
                let decoded = std::str::from_utf8(&c[..]).map_err(|e| {
                    anyhow::anyhow!(
                        "{label} could not be parsed: a CDATA section is not \
                         valid UTF-8 ({e})"
                    )
                })?;
                let chars = decoded.chars().count();
                out.check_pending(paragraph_chars + chars + cell_chars + row_chars + 1, label)?;
                push_word_text(&mut paragraph, decoded, row_depth > 0);
                paragraph_chars += chars;
            }
            Event::End(ref e) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                } else {
                    match e.local_name().as_ref() {
                        b"t" => in_text_run = false,
                        b"p" => {
                            let trimmed = paragraph.trim();
                            if !trimmed.is_empty() {
                                if row_depth > 0 {
                                    let separator = usize::from(!cell.is_empty());
                                    let chars = trimmed.chars().count() + separator;
                                    out.check_pending(cell_chars + chars + row_chars + 1, label)?;
                                    if separator == 1 {
                                        cell.push(' ');
                                    }
                                    cell.push_str(trimmed);
                                    cell_chars += chars;
                                } else {
                                    out.push_line(trimmed, label)?;
                                }
                            }
                            paragraph.clear();
                            paragraph_chars = 0;
                        }
                        // An unmatched `</w:tc>` closes nothing, and only the
                        // outermost cell closes a column, so an inner table's
                        // cells stay part of the text of the cell that
                        // contains them rather than adding columns of their
                        // own or erasing the text beside them.
                        b"tc" if cell_depth > 0 => {
                            cell_depth -= 1;
                            if cell_depth == 0 {
                                let trimmed = cell.trim();
                                let separator = usize::from(cells_in_row > 0);
                                let chars = trimmed.chars().count() + separator;
                                out.check_pending(row_chars + chars + 1, label)?;
                                if separator == 1 {
                                    row.push('\t');
                                }
                                row.push_str(trimmed);
                                row_chars += chars;
                                cells_in_row += 1;
                                cell.clear();
                                cell_chars = 0;
                            }
                        }
                        b"tr" => {
                            row_depth = row_depth.saturating_sub(1);
                            if row_depth == 0 {
                                out.push_line(&row, label)?;
                                row.clear();
                                row_chars = 0;
                                cells_in_row = 0;
                                cell_depth = 0;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    // The reader was given one byte of headroom past the cap; if it used
    // every byte, the part is larger than the cap and the text above is a
    // prefix of an unknown whole. Refuse instead of answering from it.
    if reader.into_inner().into_inner().limit() == 0 {
        bail!(
            "{label} has a {DOCUMENT_PART} that inflates to more than \
             {MAX_DECOMPRESSED_BYTES} bytes of uncompressed content, over the limit"
        );
    }
    if let Some(err) = malformed {
        return Err(err);
    }

    let (text, sections) = out.finish();
    let size_bytes = std::fs::metadata(path)
        .with_context(|| format!("{label} could not be read"))?
        .len();
    Ok(ParsedDocument {
        format: "docx".to_string(),
        document_type: "word".to_string(),
        size_bytes,
        text,
        sections,
        sheets: Vec::new(),
    })
}

/// Renders streamed cells as tab-separated rows. Gaps of up to
/// [`EMPTY_COLUMNS_INLINE`] empty columns become bare tabs; wider column
/// gaps and every skipped row become a bracketed marker, so a cell far from
/// the rest of the sheet costs a marker instead of a sea of separators.
#[derive(Debug, Default)]
pub struct SheetBodyBuilder {
    out: String,
    chars: usize,
    last_row: Option<u32>,
    last_col: u32,
}

impl SheetBodyBuilder {
    fn push_str(&mut self, s: &str) {
        self.out.push_str(s);
        self.chars += s.chars().count();
    }

    fn push_gap(&mut self, empty_columns: u32) {
        if empty_columns == 0 {
            return;
        }
        if empty_columns <= EMPTY_COLUMNS_INLINE {
            for _ in 0..empty_columns {
                self.push_str("\t");
            }
        } else {
            self.push_str(&format!("[{empty_columns} empty columns]\t"));
        }
    }

    /// Appends one cell at zero-based (`row`, `col`). Cells normally arrive
    /// in row-major order, as the cell reader yields them from a well-formed
    /// sheet; out-of-order cells are rendered in arrival order without
    /// markers and never grow the text beyond their own values.
    pub fn push(&mut self, row: u32, col: u32, value: &str) {
        match self.last_row {
            None => self.push_gap(col),
            Some(last) if last == row => {
                self.push_str("\t");
                self.push_gap(col.saturating_sub(self.last_col + 1));
            }
            Some(last) => {
                self.push_str("\n");
                let skipped = row.saturating_sub(last + 1);
                if skipped > 0 {
                    self.push_str(&format!("[{skipped} empty rows]\n"));
                }
                self.push_gap(col);
            }
        }
        self.push_str(value);
        self.last_row = Some(row);
        self.last_col = col;
    }

    /// The body text and its character count.
    pub fn finish(self) -> (String, usize) {
        (self.out, self.chars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sheet_body_builder_renders_rows_gaps_and_markers() {
        let mut b = SheetBodyBuilder::default();
        b.push(0, 0, "Item");
        b.push(0, 1, "Amount");
        b.push(1, 0, "Rent");
        b.push(1, 3, "1800");
        b.push(4, 2, "note");
        b.push(4, 16_383, "far");
        b.push(1_048_575, 0, "end");
        let (text, chars) = b.finish();
        assert_eq!(
            text,
            "Item\tAmount\nRent\t\t\t1800\n[2 empty rows]\n\t\tnote\t[16380 empty columns]\tfar\n\
             [1048570 empty rows]\nend"
        );
        assert_eq!(chars, text.chars().count());
        assert!(chars < 120, "two corner cells cost markers, not a grid");
    }

    #[test]
    fn test_sheet_body_builder_leading_gap_inline_or_marker() {
        let mut b = SheetBodyBuilder::default();
        b.push(0, EMPTY_COLUMNS_INLINE, "x");
        let (text, _) = b.finish();
        assert_eq!(text, "\t".repeat(EMPTY_COLUMNS_INLINE as usize) + "x");
        let mut b = SheetBodyBuilder::default();
        b.push(0, EMPTY_COLUMNS_INLINE + 1, "x");
        let (text, _) = b.finish();
        assert_eq!(text, "[9 empty columns]\tx");
    }

    #[test]
    fn test_word_text_builder_empty_document_produces_no_text_or_sections() {
        let mut builder = WordTextBuilder::new(100);
        // A document of blank paragraphs pushes only empty lines.
        for _ in 0..10_000 {
            builder.push_line("", "empty.docx").unwrap();
            builder.push_line("   \t ", "empty.docx").unwrap();
        }
        assert_eq!(builder.chars(), 0);
        let (text, sections) = builder.finish();
        assert!(text.is_empty(), "{text:?}");
        assert!(sections.is_empty(), "{sections:?}");
    }

    #[test]
    fn test_word_text_builder_accepts_a_budget_hit_exactly_and_refuses_one_more() {
        // Two lines of four characters each cost five with their newlines.
        let mut builder = WordTextBuilder::new(10);
        builder.push_line("abcd", "b.docx").unwrap();
        builder.push_line("efgh", "b.docx").unwrap();
        assert_eq!(builder.chars(), 10);

        let err = builder.push_line("i", "b.docx").unwrap_err();
        assert!(
            err.to_string()
                .starts_with("b.docx extracted to more than 10 characters"),
            "{err}"
        );
        // The refused line left nothing behind.
        assert_eq!(builder.chars(), 10);
        let (text, _) = builder.finish();
        assert_eq!(text, "abcd\nefgh\n");
    }

    #[test]
    fn test_word_text_builder_check_pending_refuses_before_a_buffer_grows() {
        let builder = WordTextBuilder::new(10);
        assert!(builder.check_pending(10, "b.docx").is_ok());
        let err = builder.check_pending(11, "b.docx").unwrap_err();
        assert!(err.to_string().contains("over the limit"), "{err}");
    }

    #[test]
    fn test_word_text_builder_groups_paragraphs_into_sections_with_offsets() {
        let mut builder = WordTextBuilder::new(MAX_EXTRACTED_CHARS);
        for i in 0..WORD_PARAGRAPHS_PER_SECTION * 2 + 1 {
            builder.push_line(&format!("line {i}"), "b.docx").unwrap();
        }
        let (text, sections) = builder.finish();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].offset, Some(0));
        assert_eq!(sections[0].index, 0);
        assert_eq!(sections[0].kind, "paragraph");
        // Offsets and spans tile the text exactly.
        let total: usize = sections.iter().map(|s| s.chars).sum();
        assert_eq!(total, text.chars().count());
        assert_eq!(sections[1].offset, Some(sections[0].chars), "{sections:?}");
        assert_eq!(sections[2].chars, "line 20\n".chars().count());
    }

    #[test]
    fn test_word_text_builder_caps_the_section_table_and_keeps_offsets_truthful() {
        let mut builder = WordTextBuilder::new(MAX_EXTRACTED_CHARS);
        let lines = (MAX_WORD_SECTIONS + 5) * WORD_PARAGRAPHS_PER_SECTION;
        for _ in 0..lines {
            builder.push_line("x", "b.docx").unwrap();
        }
        let (text, sections) = builder.finish();
        assert_eq!(sections.len(), MAX_WORD_SECTIONS);
        // The last section absorbed the tail, so the table still spans the
        // whole text and every reported offset is real.
        let total: usize = sections.iter().map(|s| s.chars).sum();
        assert_eq!(total, text.chars().count());
        for section in &sections {
            let offset = section.offset.unwrap();
            assert!(offset < text.chars().count(), "{section:?}");
        }
        assert!(
            sections[MAX_WORD_SECTIONS - 1].chars > sections[0].chars,
            "{:?}",
            sections[MAX_WORD_SECTIONS - 1]
        );
    }

    #[test]
    fn test_is_skipped_word_subtree_covers_revisions_and_field_codes() {
        for name in [
            b"del".as_slice(),
            b"moveFrom",
            b"instrText",
            b"delInstrText",
            b"Fallback",
        ] {
            assert!(is_skipped_word_subtree(name), "{name:?}");
        }
        for name in [b"p".as_slice(), b"t", b"tr", b"tc", b"moveTo", b"ins"] {
            assert!(!is_skipped_word_subtree(name), "{name:?}");
        }
    }

    #[test]
    fn test_push_word_text_replaces_the_breaks_text_could_forge() {
        // Outside a row a tab is ordinary text; a newline never is.
        let mut out = String::new();
        push_word_text(&mut out, "a\tb\nc\rd", false);
        assert_eq!(out, "a\tb c d");

        // Inside a row the tab is the column separator, so it goes too.
        let mut out = String::new();
        push_word_text(&mut out, "a\tb\nc\rd", true);
        assert_eq!(out, "a b c d");

        // Every replacement is one character for one, so counts stay exact.
        let source = "x\t\n\r\u{2603}";
        let mut out = String::new();
        push_word_text(&mut out, source, true);
        assert_eq!(out.chars().count(), source.chars().count());
    }

    #[test]
    fn test_word_text_builder_push_line_normalizes_embedded_newlines() {
        let mut builder = WordTextBuilder::new(MAX_EXTRACTED_CHARS);
        builder.push_line("one\ntwo\rthree", "b.docx").unwrap();
        builder.push_line("plain", "b.docx").unwrap();
        let (text, sections) = builder.finish();
        assert_eq!(text, "one two three\nplain\n");
        // Exactly two lines, so the section span still tiles the text.
        assert_eq!(text.matches('\n').count(), 2);
        assert_eq!(sections[0].chars, text.chars().count());
    }

    #[test]
    fn test_contain_panics_turns_a_panic_into_an_error() {
        let err = contain_panics("'x'", || -> Result<()> { panic!("boom") }).unwrap_err();
        assert_eq!(
            err.to_string(),
            "'x' could not be parsed: the parser panicked (boom)"
        );
        let err = contain_panics("'x'", || -> Result<()> {
            panic!("{}", String::from("formatted"))
        })
        .unwrap_err();
        assert!(err.to_string().ends_with("(formatted)"), "{err}");
        assert_eq!(contain_panics("'x'", || Ok(7)).unwrap(), 7);
    }

    /// Each sheet's header and closing blank line count against the budget
    /// with its body, so a long sheet name cannot carry the text past it.
    #[test]
    fn test_extract_workbook_counts_sheet_headers_against_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("one.xlsx");
        let mut workbook = rust_xlsxwriter::Workbook::new(path.to_str().unwrap());
        let sheet = workbook.add_worksheet();
        sheet.set_name("Data").unwrap();
        sheet.write_string_only(0, 0, "x").unwrap();
        workbook.close().unwrap();

        // "=== Sheet: Data ===\n", "x" and "\n\n" make 23 characters.
        let budget = |max_chars| ExtractBudget {
            max_chars,
            max_cells: MAX_CELLS,
        };
        let parsed = extract_workbook(&path, "'one'", budget(23)).unwrap();
        assert_eq!(parsed.text, "=== Sheet: Data ===\nx\n\n");
        let err = extract_workbook(&path, "'one'", budget(22)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "'one' extracted to more than 22 characters, over the limit"
        );
    }

    #[test]
    fn test_path_label_quotes_the_path() {
        assert_eq!(path_label(Path::new("/tmp/q3.xlsx")), "'/tmp/q3.xlsx'");
    }

    #[test]
    fn round_trip_document_section() {
        let section = DocumentSection {
            index: 2,
            kind: "sheet".to_string(),
            name: Some("Q3".to_string()),
            offset: Some(40),
            chars: 12,
        };
        let json = serde_json::to_string(&section).unwrap();
        assert_eq!(
            serde_json::from_str::<DocumentSection>(&json).unwrap(),
            section
        );

        // Absent fields are left out of the JSON and come back absent.
        let bare = DocumentSection {
            name: None,
            offset: None,
            ..section
        };
        let json = serde_json::to_string(&bare).unwrap();
        assert!(!json.contains("name") && !json.contains("offset"), "{json}");
        assert_eq!(
            serde_json::from_str::<DocumentSection>(&json).unwrap(),
            bare
        );
    }

    #[test]
    fn test_check_extension_accepts_listed_kinds_and_explains_legacy_xls() {
        let accepted = ["xlsx", "csv"];
        let ext = check_extension(Path::new("q3.XLSX"), "'q3'", &accepted).unwrap();
        assert_eq!(ext, "xlsx");

        let err = check_extension(Path::new("old.xls"), "'old'", &accepted).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("'old' is a legacy .xls workbook"),
            "{err}"
        );
        let err = check_extension(Path::new("notes.doc"), "'notes'", &accepted).unwrap_err();
        assert_eq!(
            err.to_string(),
            "'notes' has unsupported extension 'doc' (supported: xlsx, csv)"
        );
        let err = check_extension(Path::new("noext"), "'noext'", &accepted).unwrap_err();
        assert!(
            err.to_string().contains("unsupported extension ''"),
            "{err}"
        );
    }

    #[test]
    fn test_check_source_file_refuses_directories_missing_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let err = check_source_file(dir.path(), "'dir'", 10).unwrap_err();
        assert_eq!(err.to_string(), "'dir' is not a regular file");

        let missing = dir.path().join("missing.csv");
        let err = check_source_file(&missing, "'missing'", 10).unwrap_err();
        assert!(
            err.to_string().starts_with("'missing' could not be read"),
            "{err}"
        );

        let path = dir.path().join("eleven.csv");
        std::fs::write(&path, b"0123456789a").unwrap();
        assert_eq!(check_source_file(&path, "'eleven'", 11).unwrap(), 11);
        let err = check_source_file(&path, "'eleven'", 10).unwrap_err();
        assert_eq!(
            err.to_string(),
            "'eleven' is 11 bytes, over the 10-byte limit"
        );
    }

    #[test]
    fn test_extract_csv_reads_utf8_text_as_one_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rows.csv");
        std::fs::write(&path, "a,b\n1,\u{fc}\n").unwrap();
        let parsed = extract_csv(&path, "'rows'", ExtractBudget::DEFAULT).unwrap();
        assert_eq!(parsed.text, "a,b\n1,\u{fc}\n");
        assert_eq!(
            (parsed.format.as_str(), parsed.document_type.as_str()),
            ("csv", "excel")
        );
        assert_eq!(parsed.size_bytes, 9);
        assert_eq!(
            parsed.sections,
            vec![DocumentSection {
                index: 0,
                kind: "csv".to_string(),
                name: None,
                offset: Some(0),
                chars: 8,
            }]
        );

        let empty = dir.path().join("empty.csv");
        std::fs::write(&empty, "").unwrap();
        let parsed = extract_csv(&empty, "'empty'", ExtractBudget::DEFAULT).unwrap();
        assert!(parsed.sections.is_empty());
    }

    #[test]
    fn test_extract_csv_refuses_invalid_utf8_and_text_over_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.csv");
        std::fs::write(&path, b"ok,\xff\n").unwrap();
        let err = extract_csv(&path, "'latin1'", ExtractBudget::DEFAULT).unwrap_err();
        assert_eq!(
            err.to_string(),
            "'latin1' is not valid UTF-8 (first invalid byte at offset 3)"
        );

        let path = dir.path().join("long.csv");
        std::fs::write(&path, "abcdef").unwrap();
        let budget = ExtractBudget {
            max_chars: 5,
            max_cells: MAX_CELLS,
        };
        let err = extract_csv(&path, "'long'", budget).unwrap_err();
        assert!(
            err.to_string().contains("over the 5 character limit"),
            "{err}"
        );
    }

    /// The read stops one byte past the file cap, so a file that grew
    /// after `check_source_file` (or a caller that skipped it) is refused
    /// rather than read whole.
    #[test]
    fn test_extract_csv_refuses_a_file_over_the_cap_without_the_size_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.csv");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_DOCUMENT_BYTES + 1)
            .unwrap();
        let err = extract_csv(&path, "'big'", ExtractBudget::DEFAULT).unwrap_err();
        assert_eq!(
            err.to_string(),
            "'big' is more than 10485760 bytes, over the limit"
        );
    }

    /// The size is checked before the container: an oversized file is
    /// refused for its size, and a small broken one by the preflight
    /// rather than by calamine.
    #[test]
    fn test_read_document_checks_the_size_and_then_the_container() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.xlsx");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX_DOCUMENT_BYTES + 1)
            .unwrap();
        let err = read_document(&big, "'big'", ExtractBudget::DEFAULT).unwrap_err();
        assert!(
            err.to_string().contains("over the 10485760-byte limit"),
            "{err}"
        );

        let broken = dir.path().join("broken.xlsx");
        std::fs::write(&broken, b"not a zip").unwrap();
        let err = read_document(&broken, "'broken'", ExtractBudget::DEFAULT).unwrap_err();
        assert!(
            err.to_string().contains("not a valid xlsx container"),
            "{err}"
        );
    }

    #[test]
    fn test_read_document_dispatches_each_kind_and_refuses_legacy_xls() {
        use crate::office::test_fixtures::{write_docx_lines, write_far_apart_workbook};

        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("rows.csv");
        std::fs::write(&csv, "a\n").unwrap();
        let parsed = read_document(&csv, "'rows'", ExtractBudget::DEFAULT).unwrap();
        assert_eq!(parsed.format, "csv");

        let docx = write_docx_lines(dir.path(), 1);
        let parsed = read_document(&docx, "'lines'", ExtractBudget::DEFAULT).unwrap();
        assert_eq!(
            (parsed.format.as_str(), parsed.text.as_str()),
            ("docx", "line 0\n")
        );

        let xlsx = write_far_apart_workbook(dir.path());
        let parsed = read_document(&xlsx, "'far'", ExtractBudget::DEFAULT).unwrap();
        assert_eq!(parsed.format, "xlsx");
        assert_eq!(parsed.sheets.len(), 1);

        let xls = dir.path().join("old.xls");
        std::fs::write(&xls, b"junk").unwrap();
        let err = read_document(&xls, "'old'", ExtractBudget::DEFAULT).unwrap_err();
        assert!(err.to_string().contains("legacy .xls"), "{err}");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // ---------------------------------------------------------------
    // SheetBodyBuilder: panic safety plus the tab/gap-marker rendering
    // invariants for well-formed (row-major, in-order) cell streams.
    // ---------------------------------------------------------------

    proptest! {
        /// Never panics for an arbitrary stream of (row, col, value) pushes,
        /// including out-of-order and repeated positions.
        #[test]
        fn sheet_body_builder_never_panics(
            cells in proptest::collection::vec(
                (0u32..20, 0u32..20, "[a-zA-Z0-9]{0,8}"),
                0..30,
            ),
        ) {
            let mut builder = SheetBodyBuilder::default();
            for (row, col, value) in cells {
                builder.push(row, col, &value);
            }
            let _ = builder.finish();
        }

        /// For a row-major, strictly increasing, gap-free stream of single
        /// non-empty values, the body renders as exactly one line per row
        /// with values tab-separated in column order, and the reported
        /// character count matches the rendered text's own length.
        #[test]
        fn sheet_body_builder_renders_dense_grid_as_tab_separated_rows(
            rows in 1..6u32,
            cols in 1..6u32,
        ) {
            let mut builder = SheetBodyBuilder::default();
            let mut expected_lines = Vec::new();
            for row in 0..rows {
                let mut line_cells = Vec::new();
                for col in 0..cols {
                    let value = format!("r{row}c{col}");
                    builder.push(row, col, &value);
                    line_cells.push(value);
                }
                expected_lines.push(line_cells.join("\t"));
            }
            let (text, chars) = builder.finish();
            prop_assert_eq!(text.clone(), expected_lines.join("\n"));
            prop_assert_eq!(text.chars().count(), chars);
        }
    }

    // ---------------------------------------------------------------
    // WordTextBuilder: direct property tests (exercised indirectly through
    // extract_word above; these drive the builder's own public API, as
    // named explicitly in the lane's target list).
    // ---------------------------------------------------------------

    proptest! {
        /// Never panics for an arbitrary sequence of `push_line` calls.
        #[test]
        fn word_text_builder_push_line_never_panics(
            lines in proptest::collection::vec("(?s).{0,64}", 0..20),
            max_chars in 16..4096usize,
        ) {
            let mut builder = WordTextBuilder::new(max_chars);
            for line in &lines {
                let _ = builder.push_line(line, "doc.docx");
            }
        }

        /// A whitespace-only (or empty) line is always dropped -- it grows
        /// neither the text nor the section table nor the char count.
        #[test]
        fn word_text_builder_drops_whitespace_only_lines(
            whitespace in prop_oneof![Just(""), Just("   "), Just("\t"), Just("  \t ")],
            max_chars in 16..4096usize,
        ) {
            let mut builder = WordTextBuilder::new(max_chars);
            let before = builder.chars();
            builder.push_line(whitespace, "doc.docx").unwrap();
            prop_assert_eq!(builder.chars(), before);
            let (text, sections) = builder.finish();
            prop_assert!(text.is_empty());
            prop_assert!(sections.is_empty());
        }

        /// A non-empty (post-trim) line is always accepted whole when it
        /// fits the budget: it becomes exactly one line of `finish().0`,
        /// with no embedded `\n`/`\r` surviving from the input (both are
        /// normalized to spaces, per `push_line`'s own documented
        /// invariant), and `chars()` grows by exactly the line's length
        /// plus its trailing newline.
        #[test]
        fn word_text_builder_accepts_a_fitting_non_blank_line_as_exactly_one_output_line(
            line in "[a-zA-Z0-9 ]{1,40}",
        ) {
            prop_assume!(!line.trim().is_empty());
            let mut builder = WordTextBuilder::new(4096);
            builder.push_line(&line, "doc.docx").unwrap();
            let expected_chars = line.chars().count() + 1;
            prop_assert_eq!(builder.chars(), expected_chars);
            let (text, sections) = builder.finish();
            prop_assert_eq!(text, format!("{line}\n"));
            prop_assert_eq!(sections.len(), 1);
            prop_assert_eq!(sections[0].chars, expected_chars);
        }

        /// The section table never exceeds `MAX_WORD_SECTIONS`, however
        /// many lines are pushed.
        #[test]
        fn word_text_builder_section_table_never_exceeds_the_cap(
            count in 0..64usize,
        ) {
            let mut builder = WordTextBuilder::new(usize::MAX / 2);
            for i in 0..count {
                builder.push_line(&format!("line {i}"), "doc.docx").unwrap();
            }
            let (_text, sections) = builder.finish();
            prop_assert!(sections.len() <= MAX_WORD_SECTIONS);
        }

        /// `check_pending` refuses before a buffer grows past the budget,
        /// and once it refuses, `push_line` with content that would exceed
        /// the same budget also refuses -- the two must agree, since
        /// `push_line` calls `check_pending` internally before writing.
        #[test]
        fn word_text_builder_check_pending_and_push_line_agree(
            max_chars in 1..64usize,
            line in "[a-zA-Z0-9 ]{1,80}",
        ) {
            let builder = WordTextBuilder::new(max_chars);
            let pending = line.chars().count() + 1;
            let precheck = builder.check_pending(pending, "doc.docx").is_ok();

            let mut builder = WordTextBuilder::new(max_chars);
            let pushed = builder.push_line(&line, "doc.docx").is_ok();
            prop_assert_eq!(precheck, pushed);
        }
    }
}
