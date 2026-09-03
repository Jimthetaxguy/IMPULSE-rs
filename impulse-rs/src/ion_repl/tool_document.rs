//! `document_read` as a `ReplTool`: bounded, pageable document analysis
//! for Ion.
//!
//! The `office` module can already parse spreadsheets and Word files, and
//! `src/tooling/document` exposes that to the CLI and daemon, but neither
//! was reachable from Ion's tool loop, and both return whole documents in
//! one unbounded payload. A model working inside a loop contract
//! (ADR-0017) needs the opposite shape: a short outline first (sections
//! with their character offsets), then windows of content it can page
//! through with an explicit `offset`, so one large invoice or workbook
//! never floods the context budget and the model can jump straight to the
//! part it needs instead of paging exhaustively.
//!
//! Documents come from third parties, so a hostile file must never take
//! the `ion` process down. Bounds, in the order they apply:
//!
//! - the source file is refused above [`MAX_DOCUMENT_BYTES`];
//! - an `xlsx`/`docx` zip container is inflated once, entry by entry,
//!   through [`MAX_DECOMPRESSED_BYTES`] before any parser runs, so a forged
//!   central directory cannot hide a decompression bomb;
//! - workbooks are not handed to the dense-grid parser at all: cells are
//!   streamed one at a time through calamine's cell reader and written into
//!   this module's own text under [`MAX_EXTRACTED_CHARS`] and [`MAX_CELLS`],
//!   so two cells at opposite corners of a sheet or thousands of references
//!   to one huge shared string cost only what they render; a chart or
//!   dialog sheet holds no cells and is skipped rather than failing the
//!   whole workbook;
//! - Word documents are likewise streamed: `word/document.xml` is read
//!   event by event through quick-xml into one line per non-empty
//!   paragraph and one line per table row, so the docx object tree, which
//!   is many times the size of the XML, is never built. Every buffer that
//!   holds text on its way to the output is checked against
//!   [`MAX_EXTRACTED_CHARS`] *before* it grows, so no long-lived buffer
//!   overshoots on one event; the outline is capped at
//!   [`MAX_WORD_SECTIONS`] rows; and an output line is always exactly one
//!   paragraph or one table row, so document text can never contribute a
//!   line or column break of its own. quick-xml's own event buffer and
//!   open-element stack scale with the part rather than the budget, so
//!   working memory for this path is bounded by roughly twice
//!   [`MAX_DECOMPRESSED_BYTES`], which the part cap enforces here even
//!   when [`preflight_container`] has not run;
//! - `csv` text is checked against [`MAX_EXTRACTED_CHARS`] after parsing,
//!   where the parser's memory is bounded by the 10 MiB file cap;
//! - legacy `xls` is refused because its binary format has no streaming
//!   reader and cannot be bounded the same way;
//! - the content window is capped at [`MAX_CHARS_CAP`] characters; the
//!   rendered section table at [`MAX_RENDERED_SECTIONS`] rows, shown only on
//!   the first page or in outline mode;
//! - parsing runs on the blocking pool so the loop contract's wall clock can
//!   still fire.
//!
//! These bound the parser's inputs; they are not an OS sandbox.
//!
//! This tool is read-only, so it stays outside `CONFIRMATION_REQUIRED_TOOLS`
//! like `file_read`. Paths resolve against the REPL's launch directory but
//! absolute paths are accepted: the everyday-assistance case is a document
//! that lives outside any repository.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::office::{self, ExtractionResult, OfficeFormat};

use super::tools::{ReplTool, ToolOutcome};
use super::ReplContext;

/// Characters returned per call when the model does not ask for a size.
pub const DEFAULT_MAX_CHARS: usize = 12_000;
/// Hard ceiling on characters returned per call, whatever the model asks.
pub const MAX_CHARS_CAP: usize = 32_000;
/// Largest source file the tool will read.
pub const MAX_DOCUMENT_BYTES: u64 = 10 * 1024 * 1024;
/// Largest total size the entries of an `xlsx`/`docx` zip container may
/// inflate to. Every entry is inflated once through this cap before a parser
/// runs.
pub const MAX_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
/// Largest text the tool will extract from one document, in characters.
pub const MAX_EXTRACTED_CHARS: usize = 16_000_000;
/// Largest number of non-empty cells the tool will stream from a workbook.
pub const MAX_CELLS: u64 = 2_000_000;
/// Most section rows rendered for the model; the payload keeps them all.
pub const MAX_RENDERED_SECTIONS: usize = 32;
/// Empty columns inside a row rendered as bare tabs; wider gaps become a
/// marker so a cell far to the right cannot inflate the text.
pub const EMPTY_COLUMNS_INLINE: u32 = 8;

const SHEET_HEADER_PREFIX: &str = "=== Sheet: ";
const SHEET_HEADER_SUFFIX: &str = " ===";
const SUPPORTED_FORMATS: &str = "xlsx, csv, docx";

pub struct DocumentReadTool;

fn default_max_chars() -> usize {
    DEFAULT_MAX_CHARS
}

/// Validated arguments for one `document_read` call. Deserializes from the
/// same shape the schema advertises, so a bare `{"path": ...}` is valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentReadRequest {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(default)]
    pub outline: bool,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
}

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

/// The structured payload every successful call returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentWindow {
    pub path: String,
    pub format: String,
    pub document_type: String,
    pub size_bytes: u64,
    pub sections: Vec<DocumentSection>,
    /// The sheet the window was cut from, when `sheet` was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Characters in the selected text (whole document or one sheet).
    pub total_chars: usize,
    pub offset: usize,
    pub returned_chars: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    /// Absent in outline mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// One non-empty worksheet as this module rendered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetBody {
    pub name: String,
    /// Position in the workbook, counting empty sheets.
    pub index: usize,
    pub body: String,
}

/// Everything `document_read` knows about one file, in the character
/// coordinates of `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocument {
    pub format: String,
    pub document_type: String,
    pub size_bytes: u64,
    /// The whole-document text the model pages through.
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

#[async_trait]
impl ReplTool for DocumentReadTool {
    fn name(&self) -> &'static str {
        "document_read"
    }

    fn usage(&self) -> &'static str {
        "document_read {\"path\": \"...\", \"sheet\": \"...\", \"outline\": false, \
         \"offset\": 0, \"max_chars\": 12000} -- read a spreadsheet or Word document \
         (xlsx/csv/docx) as text, paged by character offset"
    }

    fn json_schema(&self) -> Value {
        json!({
            "name": "document_read",
            "description": format!(
                "Read a spreadsheet (xlsx, csv) or Word document (docx) as plain text. \
                 Read-only; files up to {} MiB. The tool loop allows only a few calls per \
                 turn, so do not page through a large document exhaustively: start with \
                 outline=true to learn total_chars and the sections with their offsets, then \
                 read only the windows you need (max_chars up to {}) and answer from what you \
                 read. Every result ends with either 'complete' or 'truncated, continue with \
                 offset=N'. Use sheet to read one worksheet (empty worksheets are omitted, \
                 as are chart, dialog, and macro sheets, which hold no cells), or \
                 pass a section's offset to jump to it.",
                MAX_DOCUMENT_BYTES / (1024 * 1024),
                MAX_CHARS_CAP
            ),
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Document path; relative paths resolve against the REPL's launch directory"
                    },
                    "sheet": {
                        "type": "string",
                        "description": "Worksheet name to read (spreadsheets only, case-insensitive; empty worksheets are omitted, as are chart, dialog, and macro sheets, which are not worksheets and hold no cells). When set, offset is relative to that sheet's text."
                    },
                    "outline": {
                        "type": "boolean",
                        "description": "Return the section table and sizes only, no content",
                        "default": false
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Character offset to start from: a section's offset from the outline, or the offset named by the previous result's continuation hint",
                        "default": 0
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": format!("Characters to return, default {DEFAULT_MAX_CHARS}, capped at {MAX_CHARS_CAP}; windows end on a line boundary when possible"),
                        "default": DEFAULT_MAX_CHARS
                    }
                },
                "required": ["path"]
            }
        })
    }

    async fn run(&self, args: Value, ctx: &ReplContext) -> Result<ToolOutcome> {
        let request = parse_request(&args)?;
        let path = resolve_document_path(&request.path, ctx)?;
        preflight_container(&path, &request.path)?;
        // Parsing is synchronous (calamine, docx-rs). Run it off the async
        // runtime so the loop contract's wall-clock timeout can still fire
        // while a large file parses.
        let window = {
            let request = request.clone();
            let path = path.clone();
            tokio::task::spawn_blocking(move || -> Result<DocumentWindow> {
                let parsed = parse_document_bounded(&path, &request.path, ExtractBudget::DEFAULT)?;
                build_window(&request, &path, &parsed)
            })
            .await
            .context("document_read parse task panicked")??
        };
        let rendered = render(&window);
        let payload =
            serde_json::to_value(&window).context("failed to serialize document window")?;
        Ok(ToolOutcome {
            rendered,
            payload,
            ok: true,
        })
    }
}

/// Validates the raw tool arguments. `max_chars` is clamped to
/// [`MAX_CHARS_CAP`] rather than rejected so a generous model request still
/// succeeds with a bounded result.
pub fn parse_request(args: &Value) -> Result<DocumentReadRequest> {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => bail!("document_read requires a non-empty 'path'"),
    };
    let sheet = match args.get("sheet") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        // A supplied-but-blank selector is a mistake, not a request for the
        // whole workbook: falling back silently would hand the model
        // unrelated data at whole-document cost.
        Some(Value::String(_)) => {
            bail!("'sheet' must not be blank; omit it to read the whole document")
        }
        Some(other) => bail!("'sheet' must be a string, got {other}"),
    };
    let outline = match args.get("outline") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(other) => bail!("'outline' must be a boolean, got {other}"),
    };
    let offset =
        match args.get("offset") {
            None | Some(Value::Null) => 0,
            Some(v) => usize::try_from(v.as_u64().ok_or_else(|| {
                anyhow::anyhow!("'offset' must be a non-negative integer, got {v}")
            })?)
            .context("'offset' is too large")?,
        };
    let max_chars = match args.get("max_chars") {
        None | Some(Value::Null) => DEFAULT_MAX_CHARS,
        Some(v) => {
            let requested = v.as_u64().ok_or_else(|| {
                anyhow::anyhow!("'max_chars' must be a positive integer, got {v}")
            })?;
            if requested == 0 {
                bail!("'max_chars' must be at least 1");
            }
            usize::try_from(requested)
                .unwrap_or(MAX_CHARS_CAP)
                .min(MAX_CHARS_CAP)
        }
    };
    Ok(DocumentReadRequest {
        path,
        sheet,
        outline,
        offset,
        max_chars,
    })
}

/// Resolves `raw` against `ctx.repo_root` when relative and checks it names
/// an existing regular file in an accepted format under
/// [`MAX_DOCUMENT_BYTES`] AND inside `ctx`'s read sandbox (Stage 1 review
/// round 1, P1: `document_read` previously never consulted
/// `ReplContext::sandbox_tool_context` at all -- an absolute path, or a
/// relative `../` traversal, escaped the sandbox with no confirmation gate
/// and no path check, unlike every bridged tool. This tool is ungated by
/// design (read-only, like `file_read`), so the sandbox check here is the
/// *only* enforcement point -- it must run before any file is opened, not
/// just before it's parsed). Error messages lead with the path the caller
/// supplied, so two different bad paths never share an error signature.
pub fn resolve_document_path(raw: &str, ctx: &ReplContext) -> Result<PathBuf> {
    resolve_document_path_with_cap(raw, ctx, MAX_DOCUMENT_BYTES)
}

/// [`resolve_document_path`] with an explicit size cap; the test seam.
pub fn resolve_document_path_with_cap(
    raw: &str,
    ctx: &ReplContext,
    max_bytes: u64,
) -> Result<PathBuf> {
    let repo_root = ctx.effective_repo_root();
    let candidate = PathBuf::from(raw);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        repo_root.join(candidate)
    };
    let tool_ctx = ctx.sandbox_tool_context();
    if !tool_ctx.is_path_allowed(&path, false) {
        bail!(
            "document_read: '{raw}' resolves outside the session's read sandbox \
             (repo root plus any /allow grants); use /allow to grant access first \
             if you trust this path"
        );
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    if ext.eq_ignore_ascii_case("xls") {
        bail!(
            "document_read: '{raw}' is a legacy .xls workbook, which this tool does not accept \
             because its binary format cannot be bounded before parsing; convert it to .xlsx"
        );
    }
    if !OfficeFormat::from_extension(&ext).is_readable() {
        bail!(
            "document_read: '{raw}' has unsupported extension '{ext}' (supported: {SUPPORTED_FORMATS})"
        );
    }
    let metadata = std::fs::metadata(&path).with_context(|| {
        format!(
            "document_read: '{raw}' not found (resolved to '{}')",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        bail!("document_read: '{raw}' is a directory, not a file");
    }
    if metadata.len() > max_bytes {
        bail!(
            "document_read: '{raw}' is {} bytes, over the {max_bytes}-byte limit",
            metadata.len()
        );
    }
    Ok(path)
}

/// Case folding for sheet-name matching: Unicode lowercasing plus the
/// Latin multi-character folds that lowercasing alone misses, so `Straße`,
/// `STRAẞE`, and `STRASSE` all compare equal. Dotless `ı` is left alone (it
/// folds to itself in Unicode), so Turkish lowercase names are not conflated
/// with their dotted counterparts; uppercase `I` still lowercases to `i` as
/// in every non-Turkic locale. Folds outside Latin script that expand to
/// several characters and normalization are not applied.
pub fn fold_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.to_lowercase().chars() {
        match ch {
            'ß' => out.push_str("ss"),
            'ſ' => out.push('s'),
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            'ﬅ' | 'ﬆ' => out.push_str("st"),
            other => out.push(other),
        }
    }
    out
}

/// Inflates every entry of an `xlsx`/`docx` zip container once through
/// [`MAX_DECOMPRESSED_BYTES`] before any parser sees it, so neither a
/// forged central directory nor a highly compressible entry can balloon
/// during parsing. Other formats (`csv`) are not containers and pass.
pub fn preflight_container(path: &Path, raw: &str) -> Result<()> {
    preflight_container_with_limit(path, raw, MAX_DECOMPRESSED_BYTES)
}

/// [`preflight_container`] with an explicit inflation cap; the test seam.
pub fn preflight_container_with_limit(path: &Path, raw: &str, max_uncompressed: u64) -> Result<()> {
    use std::io::Read as _;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(ext.as_str(), "xlsx" | "docx") {
        return Ok(());
    }
    let file = std::fs::File::open(path)
        .with_context(|| format!("document_read: '{raw}' could not be opened"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        anyhow::anyhow!(
            "document_read: '{raw}' could not be parsed: not a valid {ext} container ({e})"
        )
    })?;
    let mut inflated_total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| {
            anyhow::anyhow!(
                "document_read: '{raw}' could not be parsed: unreadable {ext} entry ({e})"
            )
        })?;
        let remaining = max_uncompressed.saturating_sub(inflated_total);
        let name = entry.name().to_string();
        let inflated = std::io::copy(
            &mut (&mut entry).take(remaining.saturating_add(1)),
            &mut std::io::sink(),
        )
        .with_context(|| format!("document_read: '{raw}' entry '{name}' failed to inflate"))?;
        if inflated > remaining {
            bail!(
                "document_read: '{raw}' inflates to more than {max_uncompressed} bytes of \
                 uncompressed content, over the limit"
            );
        }
        inflated_total += inflated;
    }
    Ok(())
}

/// Refuses extracted text above `max_chars`.
pub fn check_extracted_size(text: &str, raw: &str, max_chars: usize) -> Result<()> {
    let chars = text.chars().count();
    if chars > max_chars {
        bail!(
            "document_read: '{raw}' extracted to {chars} characters, over the {max_chars} \
             character limit"
        );
    }
    Ok(())
}

/// Parses one accepted document under `budget`: workbooks are streamed
/// cell by cell and Word documents event by event by this module; `csv`
/// goes through the `office` parser and is size-checked afterwards, its
/// memory already bounded by the 10 MiB file cap.
pub fn parse_document_bounded(
    path: &Path,
    raw: &str,
    budget: ExtractBudget,
) -> Result<ParsedDocument> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "xlsx" => extract_workbook(path, raw, budget),
        "docx" => extract_word(path, raw, budget),
        "csv" => {
            let parsed = office::parse_document(path)
                .map_err(|e| anyhow::anyhow!("document_read: '{raw}' could not be parsed: {e}"))?;
            check_extracted_size(&parsed.content, raw, budget.max_chars)?;
            Ok(from_extraction(parsed))
        }
        other => bail!(
            "document_read: '{raw}' has unsupported extension '{other}' (supported: {SUPPORTED_FORMATS})"
        ),
    }
}

/// Streams a workbook through calamine's cell reader into this module's own
/// text: `=== Sheet: name ===`, the sheet body, and a blank line per
/// non-empty sheet, with sections and offsets computed as the text is built.
/// The dense-grid parser is never used, so a far-off cell costs only its
/// gap marker and a shared string costs only the cells that render it.
pub fn extract_workbook(path: &Path, raw: &str, budget: ExtractBudget) -> Result<ParsedDocument> {
    use calamine::{open_workbook, DataRef, Reader, Xlsx};

    let mut workbook: Xlsx<_> = open_workbook(path)
        .map_err(|e| anyhow::anyhow!("document_read: '{raw}' could not be parsed: {e}"))?;
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
            Err(e) => bail!("document_read: '{raw}' could not be parsed: sheet '{name}': {e}"),
        };
        let mut body = SheetBodyBuilder::default();
        while let Some(cell) = reader.next_cell().map_err(|e| {
            anyhow::anyhow!("document_read: '{raw}' could not be parsed: sheet '{name}': {e}")
        })? {
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
                    "document_read: '{raw}' has more than {} non-empty cells, over the limit",
                    budget.max_cells
                );
            }
            let (row, col) = cell.get_position();
            body.push(row, col, &value);
            if cursor + body.chars > budget.max_chars {
                bail!(
                    "document_read: '{raw}' extracted to more than {} characters, over the limit",
                    budget.max_chars
                );
            }
        }
        let (body_text, body_chars) = body.finish();
        if body_text.is_empty() {
            continue;
        }
        let header = format!("{SHEET_HEADER_PREFIX}{name}{SHEET_HEADER_SUFFIX}\n");
        cursor += header.chars().count();
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
        .with_context(|| format!("document_read: '{raw}' could not be read"))?
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
    pub fn check_pending(&self, pending: usize, raw: &str) -> Result<()> {
        if self.cursor + pending > self.max_chars {
            bail!(
                "document_read: '{raw}' extracted to more than {} characters, over the limit",
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
    pub fn push_line(&mut self, line: &str, raw: &str) -> Result<()> {
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
        self.check_pending(chars, raw)?;
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
pub fn extract_word(path: &Path, raw: &str, budget: ExtractBudget) -> Result<ParsedDocument> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    use std::io::Read as _;

    const DOCUMENT_PART: &str = "word/document.xml";

    let file = std::fs::File::open(path)
        .with_context(|| format!("document_read: '{raw}' could not be opened"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        anyhow::anyhow!(
            "document_read: '{raw}' could not be parsed: not a valid docx container ({e})"
        )
    })?;
    // OPC part names compare case-insensitively, and calamine resolves
    // workbook parts the same way; a writer that emits `word/Document.xml`
    // produces a file every reader opens, so this one opens it too.
    let part = archive
        .file_names()
        .find(|name| name.eq_ignore_ascii_case(DOCUMENT_PART))
        .map(str::to_owned)
        .ok_or_else(|| {
            anyhow::anyhow!("document_read: '{raw}' could not be parsed: no {DOCUMENT_PART}")
        })?;
    let entry = archive.by_name(&part).map_err(|e| {
        anyhow::anyhow!("document_read: '{raw}' could not be parsed: no {DOCUMENT_PART} ({e})")
    })?;
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
                    "document_read: '{raw}' could not be parsed: {DOCUMENT_PART} is malformed \
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
                    out.check_pending(paragraph_chars + 1 + cell_chars + row_chars + 1, raw)?;
                    paragraph.push(if row_depth > 0 { ' ' } else { '\t' });
                    paragraph_chars += 1;
                }
                // A line break inside a paragraph becomes a space: one
                // output line is one paragraph or one row, always.
                b"br" | b"cr" => {
                    out.check_pending(paragraph_chars + 1 + cell_chars + row_chars + 1, raw)?;
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
                    anyhow::anyhow!(
                        "document_read: '{raw}' could not be parsed: bad text escape ({e})"
                    )
                })?;
                let chars = unescaped.chars().count();
                out.check_pending(paragraph_chars + chars + cell_chars + row_chars + 1, raw)?;
                push_word_text(&mut paragraph, &unescaped, row_depth > 0);
                paragraph_chars += chars;
            }
            Event::CData(ref c) if skip_depth == 0 && in_text_run => {
                let decoded = std::str::from_utf8(&c[..]).map_err(|e| {
                    anyhow::anyhow!(
                        "document_read: '{raw}' could not be parsed: a CDATA section is not \
                         valid UTF-8 ({e})"
                    )
                })?;
                let chars = decoded.chars().count();
                out.check_pending(paragraph_chars + chars + cell_chars + row_chars + 1, raw)?;
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
                                    out.check_pending(cell_chars + chars + row_chars + 1, raw)?;
                                    if separator == 1 {
                                        cell.push(' ');
                                    }
                                    cell.push_str(trimmed);
                                    cell_chars += chars;
                                } else {
                                    out.push_line(trimmed, raw)?;
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
                                out.check_pending(row_chars + chars + 1, raw)?;
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
                                out.push_line(&row, raw)?;
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
            "document_read: '{raw}' has a {DOCUMENT_PART} that inflates to more than \
             {MAX_DECOMPRESSED_BYTES} bytes of uncompressed content, over the limit"
        );
    }
    if let Some(err) = malformed {
        return Err(err);
    }

    let (text, sections) = out.finish();
    let size_bytes = std::fs::metadata(path)
        .with_context(|| format!("document_read: '{raw}' could not be read"))?
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

/// Adapts an `office` parser result into a [`ParsedDocument`], computing
/// section offsets from the parsers' fixed layouts: one span for CSV, and
/// for a paragraph chunk one paragraph per line grouped into chunks joined
/// by a blank line. Offsets are dropped when a layout cannot be matched
/// exactly. Only `csv` reaches it from this module now that Word documents
/// are streamed here; the paragraph arm stays because the adapter is
/// written against any [`ExtractionResult`], not against one caller.
pub fn from_extraction(parsed: ExtractionResult) -> ParsedDocument {
    let total_chars = parsed.content.chars().count();
    let mut sections = Vec::with_capacity(parsed.chunks.len());
    let mut cursor = 0usize;
    for chunk in &parsed.chunks {
        match chunk.chunk_type.as_str() {
            "csv" => sections.push(DocumentSection {
                index: chunk.index,
                kind: chunk.chunk_type.clone(),
                name: None,
                offset: Some(0),
                chars: total_chars,
            }),
            "paragraph" => {
                let span: usize = chunk
                    .content
                    .split("\n\n")
                    .map(|p| p.chars().count() + 1)
                    .sum();
                sections.push(DocumentSection {
                    index: chunk.index,
                    kind: chunk.chunk_type.clone(),
                    name: None,
                    offset: Some(cursor),
                    chars: span,
                });
                cursor += span;
            }
            _ => sections.push(DocumentSection {
                index: chunk.index,
                kind: chunk.chunk_type.clone(),
                name: None,
                offset: None,
                chars: chunk.content.chars().count(),
            }),
        }
    }
    let paragraph_layout_holds =
        sections.iter().all(|s| s.kind != "paragraph") || cursor == total_chars;
    if !paragraph_layout_holds {
        for section in sections.iter_mut().filter(|s| s.kind == "paragraph") {
            section.offset = None;
            section.chars = parsed
                .chunks
                .iter()
                .find(|c| c.index == section.index)
                .map(|c| c.content.chars().count())
                .unwrap_or(section.chars);
        }
    }
    ParsedDocument {
        format: parsed.metadata.format,
        document_type: parsed.document_type,
        size_bytes: parsed.metadata.size_bytes,
        text: parsed.content,
        sections,
        sheets: Vec::new(),
    }
}

/// The text a call reads from: the whole document, or one sheet when
/// `sheet` is given. Sheet selection is only meaningful for workbooks, and
/// only non-empty worksheets exist after extraction.
pub fn select_text(
    parsed: &ParsedDocument,
    sheet: Option<&str>,
) -> Result<(String, Option<String>)> {
    let Some(wanted) = sheet else {
        return Ok((parsed.text.clone(), None));
    };
    if parsed.sheets.is_empty() {
        if parsed.format == "xlsx" {
            bail!(
                "no readable worksheets in this xlsx document: every sheet is empty or is \
                 not a worksheet (empty worksheets are omitted, as are chart, dialog, and \
                 macro sheets, which hold no cells)"
            );
        }
        bail!(
            "'sheet' only applies to spreadsheets (xlsx); this {} document has no sheets",
            parsed.format
        );
    }
    let wanted_folded = fold_case(wanted);
    let found = parsed
        .sheets
        .iter()
        .find(|s| fold_case(&s.name) == wanted_folded)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "sheet '{wanted}' not found among readable worksheets (empty worksheets are \
                 omitted, as are chart, dialog, and macro sheets, which are not worksheets and \
                 hold no cells); available: {}",
                parsed
                    .sheets
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    Ok((found.body.clone(), Some(found.name.clone())))
}

/// A character window over `text`. When the window would be cut short of
/// the end, it backs up to the last complete line so a spreadsheet row or
/// paragraph is never split across pages; a single line longer than
/// `max_chars` keeps the hard cut. Offsets past the end yield an empty,
/// complete window rather than an error so a model that overshoots learns
/// it is done.
pub fn window(text: &str, offset: usize, max_chars: usize) -> (String, usize, bool, Option<usize>) {
    let total = text.chars().count();
    let start = offset.min(total);
    let mut content: String = text.chars().skip(start).take(max_chars).collect();
    let mut returned = content.chars().count();
    let truncated = start + returned < total;
    if truncated && !content.ends_with('\n') {
        if let Some(cut) = content.rfind('\n') {
            // '\n' is one byte, so `cut + 1` is a char boundary.
            content.truncate(cut + 1);
            returned = content.chars().count();
        }
    }
    let next_offset = truncated.then_some(start + returned);
    (content, returned, truncated, next_offset)
}

fn build_window(
    request: &DocumentReadRequest,
    path: &Path,
    parsed: &ParsedDocument,
) -> Result<DocumentWindow> {
    let (text, section) = select_text(parsed, request.sheet.as_deref())?;
    let total_chars = text.chars().count();
    let (content, returned_chars, truncated, next_offset) = if request.outline {
        (String::new(), 0, false, None)
    } else {
        window(&text, request.offset, request.max_chars)
    };
    Ok(DocumentWindow {
        path: path.display().to_string(),
        format: parsed.format.clone(),
        document_type: parsed.document_type.clone(),
        size_bytes: parsed.size_bytes,
        sections: parsed.sections.clone(),
        section,
        total_chars,
        offset: if request.outline {
            0
        } else {
            request.offset.min(total_chars)
        },
        returned_chars,
        truncated,
        next_offset,
        content: (!request.outline).then_some(content),
    })
}

/// The text the model sees as the tool result: a one-line header, the
/// section table (on the first page or in outline mode, capped at
/// [`MAX_RENDERED_SECTIONS`] rows), then the content window with an explicit
/// continuation hint that says how much remains.
pub fn render(window: &DocumentWindow) -> String {
    let mut out = format!(
        "document_read: {} ({}, {}, {} bytes, {} section(s), {} chars",
        window.path,
        window.format,
        window.document_type,
        window.size_bytes,
        window.sections.len(),
        window.total_chars
    );
    if let Some(section) = &window.section {
        out.push_str(&format!(", sheet \"{section}\""));
    }
    out.push_str(")\n");

    let first_page = window.content.is_none() || window.offset == 0;
    if first_page {
        for section in window.sections.iter().take(MAX_RENDERED_SECTIONS) {
            let mut row = format!("  [{}] {}", section.index, section.kind);
            if let Some(name) = &section.name {
                row.push_str(&format!(" \"{name}\""));
            }
            if let Some(offset) = section.offset {
                row.push_str(&format!(" offset={offset}"));
            }
            row.push_str(&format!(" {} chars\n", section.chars));
            out.push_str(&row);
        }
        let hidden = window.sections.len().saturating_sub(MAX_RENDERED_SECTIONS);
        if hidden > 0 {
            out.push_str(&format!("  ... and {hidden} more section(s) not listed\n"));
        }
        if window.format == "xlsx" {
            out.push_str(
                "  (empty worksheets are omitted, as are chart, dialog, and macro sheets, \
                 which are not worksheets; section indexes are workbook positions; section \
                 offsets are whole-document offsets)\n",
            );
        }
    }

    match &window.content {
        None => out.push_str(
            "(outline only; read content with offset=<section offset>, or sheet=<name> for a \
             worksheet)\n",
        ),
        Some(content) => {
            let end = window.offset + window.returned_chars;
            if window.truncated {
                let remaining = window.total_chars.saturating_sub(end);
                let per_call = window.returned_chars.max(1);
                let calls_left = remaining.div_ceil(per_call);
                out.push_str(&format!(
                    "--- content chars {}..{} of {}; truncated, {} chars remain (about {} more \
                     call(s) at this size); continue with offset={}, or raise max_chars up to \
                     {} ---\n",
                    window.offset,
                    end,
                    window.total_chars,
                    remaining,
                    calls_left,
                    window.next_offset.unwrap_or(end),
                    MAX_CHARS_CAP
                ));
            } else {
                out.push_str(&format!(
                    "--- content chars {}..{} of {}; complete ---\n",
                    window.offset, end, window.total_chars
                ));
            }
            out.push_str(content);
            if !content.ends_with('\n') {
                out.push('\n');
                if window.truncated {
                    out.push_str(&format!(
                        "[window ends mid-line; the next call continues from offset={}]\n",
                        window.next_offset.unwrap_or(end)
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{ContentChunk, ExtractionMetadata};

    fn extraction(
        format: &str,
        document_type: &str,
        content: &str,
        chunks: Vec<(&str, &str)>,
    ) -> ExtractionResult {
        ExtractionResult {
            document_type: document_type.into(),
            content: content.to_string(),
            metadata: ExtractionMetadata {
                source_path: format!("fake.{format}"),
                format: format.into(),
                size_bytes: 42,
                extracted_at: "now".into(),
            },
            chunks: chunks
                .into_iter()
                .enumerate()
                .map(|(index, (kind, body))| ContentChunk {
                    content: body.to_string(),
                    chunk_type: kind.to_string(),
                    index,
                })
                .collect(),
        }
    }

    fn workbook_doc(sheets: &[(&str, &str)]) -> ParsedDocument {
        let mut text = String::new();
        let mut cursor = 0;
        let mut sections = Vec::new();
        let mut bodies = Vec::new();
        for (index, (name, body)) in sheets.iter().enumerate() {
            let header = format!("=== Sheet: {name} ===\n");
            cursor += header.chars().count();
            sections.push(DocumentSection {
                index,
                kind: "sheet".into(),
                name: Some((*name).to_string()),
                offset: Some(cursor),
                chars: body.chars().count(),
            });
            text.push_str(&header);
            text.push_str(body);
            text.push_str("\n\n");
            cursor += body.chars().count() + 2;
            bodies.push(SheetBody {
                name: (*name).to_string(),
                index,
                body: (*body).to_string(),
            });
        }
        ParsedDocument {
            format: "xlsx".into(),
            document_type: "excel".into(),
            size_bytes: 42,
            text,
            sections,
            sheets: bodies,
        }
    }

    fn window_for(sections: Vec<DocumentSection>, content: Option<&str>) -> DocumentWindow {
        DocumentWindow {
            path: "a.xlsx".into(),
            format: "xlsx".into(),
            document_type: "excel".into(),
            size_bytes: 10,
            sections,
            section: None,
            total_chars: 30,
            offset: 0,
            returned_chars: content.map(|c| c.chars().count()).unwrap_or(0),
            truncated: false,
            next_offset: None,
            content: content.map(str::to_string),
        }
    }

    #[test]
    fn test_schema_names_tool_and_requires_path() {
        let schema = DocumentReadTool.json_schema();
        assert_eq!(schema["name"], "document_read");
        assert_eq!(schema["input_schema"]["required"], json!(["path"]));
        for key in ["path", "sheet", "outline", "offset", "max_chars"] {
            assert!(
                schema["input_schema"]["properties"].get(key).is_some(),
                "schema is missing {key}"
            );
        }
        let description = schema["description"].as_str().unwrap();
        assert!(description.contains("outline=true"), "{description}");
        assert!(
            description.contains("truncated, continue with offset=N"),
            "{description}"
        );
        assert!(description.contains("10 MiB"), "{description}");
        assert!(
            !description.contains("xls,"),
            "legacy xls is not advertised"
        );
        assert_eq!(DocumentReadTool.name(), "document_read");
        assert!(DocumentReadTool.usage().contains("document_read"));
    }

    #[test]
    fn test_parse_request_applies_defaults() {
        let request = parse_request(&json!({"path": " a.csv "})).unwrap();
        assert_eq!(
            request,
            DocumentReadRequest {
                path: "a.csv".into(),
                sheet: None,
                outline: false,
                offset: 0,
                max_chars: DEFAULT_MAX_CHARS,
            }
        );
        let from_json: DocumentReadRequest =
            serde_json::from_value(json!({"path": "a.csv"})).unwrap();
        assert_eq!(from_json, request);
    }

    #[test]
    fn test_parse_request_reads_every_field_and_clamps_max_chars() {
        let request = parse_request(&json!({
            "path": "b.xlsx", "sheet": " Q1 ", "outline": true, "offset": 12, "max_chars": 1_000_000
        }))
        .unwrap();
        assert_eq!(request.sheet.as_deref(), Some("Q1"));
        assert!(request.outline);
        assert_eq!(request.offset, 12);
        assert_eq!(request.max_chars, MAX_CHARS_CAP);
    }

    #[test]
    fn test_parse_request_rejects_bad_arguments() {
        assert!(parse_request(&json!({})).is_err());
        assert!(parse_request(&json!({"path": "  "})).is_err());
        assert!(parse_request(&json!({"path": "a.csv", "max_chars": 0})).is_err());
        assert!(parse_request(&json!({"path": "a.csv", "offset": -1})).is_err());
        assert!(parse_request(&json!({"path": "a.csv", "outline": "yes"})).is_err());
        assert!(parse_request(&json!({"path": "a.csv", "sheet": 3})).is_err());
        let err = parse_request(&json!({"path": "a.csv", "max_chars": "many"})).unwrap_err();
        assert!(err.to_string().contains("max_chars"), "{err}");
    }

    #[test]
    fn test_parse_request_rejects_blank_sheet_selector() {
        for blank in ["", "   ", "\t"] {
            let err = parse_request(&json!({"path": "a.xlsx", "sheet": blank})).unwrap_err();
            assert!(
                err.to_string().contains("must not be blank"),
                "{blank:?}: {err}"
            );
        }
        assert_eq!(
            parse_request(&json!({"path": "a.xlsx", "sheet": null}))
                .unwrap()
                .sheet,
            None
        );
    }

    #[test]
    fn test_fold_case_handles_sharp_s_ligatures_and_dotless_i() {
        assert_eq!(fold_case("Straße"), fold_case("STRASSE"));
        assert_eq!(fold_case("STRA\u{1E9E}E"), "strasse");
        assert_eq!(fold_case("Übersicht"), fold_case("ÜBERSICHT"));
        assert_eq!(fold_case("Budget"), "budget");
        assert_ne!(fold_case("Budget"), fold_case("Budgets"));
        assert_ne!(fold_case("\u{131}s\u{131}"), fold_case("isi"));
        assert_eq!(fold_case("\u{FB01}le"), "file");
        assert_eq!(fold_case("\u{17F}"), "s");
    }

    #[test]
    fn test_window_returns_everything_when_it_fits() {
        let (content, returned, truncated, next) = window("hello", 0, 10);
        assert_eq!(content, "hello");
        assert_eq!(returned, 5);
        assert!(!truncated);
        assert_eq!(next, None);
    }

    #[test]
    fn test_window_snaps_a_truncated_cut_to_the_last_complete_line() {
        let text = "item,amount\nrent,1800\nfood,300\n";
        let (content, returned, truncated, next) = window(text, 0, 18);
        assert_eq!(content, "item,amount\n");
        assert_eq!(returned, 12);
        assert!(truncated);
        assert_eq!(next, Some(12));
        let (content, _, truncated, next) = window(text, 12, 18);
        assert_eq!(content, "rent,1800\n");
        assert!(truncated);
        assert_eq!(next, Some(22));
        let (content, _, truncated, next) = window(text, 22, 18);
        assert_eq!(content, "food,300\n");
        assert!(!truncated);
        assert_eq!(next, None);
    }

    #[test]
    fn test_window_keeps_a_hard_cut_for_a_single_long_line() {
        let (content, returned, truncated, next) = window("abcdefgh", 2, 3);
        assert_eq!(content, "cde");
        assert_eq!(returned, 3);
        assert!(truncated);
        assert_eq!(next, Some(5));
    }

    #[test]
    fn test_window_past_end_is_empty_and_complete() {
        let (content, returned, truncated, next) = window("abc", 99, 3);
        assert_eq!(content, "");
        assert_eq!(returned, 0);
        assert!(!truncated);
        assert_eq!(next, None);
    }

    #[test]
    fn test_window_counts_characters_not_bytes() {
        let text = "héllo wörld";
        let (content, returned, truncated, next) = window(text, 1, 4);
        assert_eq!(content, "éllo");
        assert_eq!(returned, 4);
        assert!(truncated);
        assert_eq!(next, Some(5));
    }

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
    fn test_from_extraction_csv_and_paragraph_sections_cover_the_whole_text() {
        let csv = from_extraction(extraction(
            "csv",
            "excel",
            "a,b\n1,2\n",
            vec![("csv", "a,b\n1,2\n")],
        ));
        assert_eq!(csv.sections.len(), 1);
        assert_eq!(csv.sections[0].offset, Some(0));
        assert_eq!(csv.sections[0].chars, 8);
        assert!(csv.sheets.is_empty());

        let content = "Dear tenant,\nYour lease renews.\n[Table]\nRegards\n";
        let docx = from_extraction(extraction(
            "docx",
            "word",
            content,
            vec![
                ("paragraph", "Dear tenant,\n\nYour lease renews.\n\n[Table]"),
                ("paragraph", "Regards"),
            ],
        ));
        assert_eq!(docx.sections[0].offset, Some(0));
        assert_eq!(docx.sections[0].chars, 13 + 19 + 8);
        assert_eq!(docx.sections[1].offset, Some(40));
        assert_eq!(docx.sections[1].chars, 8);
        assert_eq!(
            docx.sections.iter().map(|s| s.chars).sum::<usize>(),
            content.chars().count()
        );
        let tail: String = content.chars().skip(40).collect();
        assert_eq!(tail, "Regards\n");
    }

    #[test]
    fn test_from_extraction_drops_paragraph_offsets_when_reconstruction_fails() {
        let docx = from_extraction(extraction(
            "docx",
            "word",
            "Something the parser would not have produced",
            vec![("paragraph", "Dear tenant,\n\nRegards")],
        ));
        assert_eq!(docx.sections[0].offset, None);
        assert_eq!(
            docx.sections[0].chars,
            "Dear tenant,\n\nRegards".chars().count()
        );
        let other = from_extraction(extraction("csv", "excel", "x", vec![("blob", "x")]));
        assert_eq!(other.sections[0].offset, None);
    }

    #[test]
    fn test_select_text_picks_sheet_case_insensitively_including_non_ascii() {
        let parsed = workbook_doc(&[("Budget", "a\tb"), ("Übersicht", "c"), ("Straße", "z")]);
        let (text, section) = select_text(&parsed, Some("übersicht")).unwrap();
        assert_eq!(text, "c");
        assert_eq!(section.as_deref(), Some("Übersicht"));
        let (text, section) = select_text(&parsed, Some("BUDGET")).unwrap();
        assert_eq!(text, "a\tb");
        assert_eq!(section.as_deref(), Some("Budget"));
        let (text, section) = select_text(&parsed, Some("STRASSE")).unwrap();
        assert_eq!(text, "z");
        assert_eq!(section.as_deref(), Some("Straße"));
        let (all, none) = select_text(&parsed, None).unwrap();
        assert_eq!(all, parsed.text);
        assert_eq!(none, None);
        // Section offsets point at each body inside the paged text.
        for (section, sheet) in parsed.sections.iter().zip(&parsed.sheets) {
            let body: String = parsed
                .text
                .chars()
                .skip(section.offset.unwrap())
                .take(section.chars)
                .collect();
            assert_eq!(body, sheet.body);
        }
    }

    #[test]
    fn test_select_text_errors_are_truthful_about_empty_and_non_spreadsheet_documents() {
        let empty_workbook = workbook_doc(&[]);
        let err = select_text(&empty_workbook, Some("Sheet1")).unwrap_err();
        assert!(err.to_string().contains("every sheet is empty"), "{err}");

        let csv = from_extraction(extraction(
            "csv",
            "excel",
            "=== Sheet: X ===\n",
            vec![("csv", "=== Sheet: X ===\n")],
        ));
        let err = select_text(&csv, Some("X")).unwrap_err();
        assert!(
            err.to_string().contains("only applies to spreadsheets"),
            "{err}"
        );

        let parsed = workbook_doc(&[("Budget", "a")]);
        let err = select_text(&parsed, Some("Missing")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Budget") && msg.contains("empty worksheets are omitted"),
            "{msg}"
        );
    }

    fn doc_ctx(repo_root: &Path) -> ReplContext {
        ReplContext {
            repo_root: repo_root.to_path_buf(),
            ..ReplContext::default()
        }
    }

    #[test]
    fn test_resolve_document_path_joins_relative_paths_and_validates() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.csv"), "x,y\n").unwrap();
        let ctx = doc_ctx(dir.path());
        let resolved = resolve_document_path("a.csv", &ctx).unwrap();
        assert_eq!(resolved, dir.path().join("a.csv"));
        let absolute = dir.path().join("a.csv");
        // An absolute path INSIDE the sandbox (repo_root) is still accepted.
        assert_eq!(
            resolve_document_path(absolute.to_str().unwrap(), &ctx).unwrap(),
            absolute
        );

        let err = resolve_document_path("missing.csv", &ctx).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.starts_with("document_read: 'missing.csv' not found"),
            "{msg}"
        );

        std::fs::write(dir.path().join("notes.pdf"), "%PDF").unwrap();
        let err = resolve_document_path("notes.pdf", &ctx).unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("document_read: 'notes.pdf'"), "{msg}");
        assert!(msg.contains("unsupported extension 'pdf'"), "{msg}");
        assert!(
            msg.contains("xlsx") && msg.contains("csv") && msg.contains("docx"),
            "{msg}"
        );

        std::fs::create_dir(dir.path().join("folder.csv")).unwrap();
        let err = resolve_document_path("folder.csv", &ctx).unwrap_err();
        assert!(err.to_string().contains("is a directory"), "{err}");

        std::fs::write(dir.path().join("old.xls"), "binary").unwrap();
        let err = resolve_document_path("old.xls", &ctx).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.starts_with("document_read: 'old.xls' is a legacy .xls workbook"),
            "{msg}"
        );
        assert!(msg.contains("convert it to .xlsx"), "{msg}");
    }

    // ------------------------------------------------------------------
    // Review round 1, P1: document_read must enforce the same sandbox as
    // the bridged tools (repo root + /allow grants), not accept any
    // absolute path or relative traversal unconditionally.
    // ------------------------------------------------------------------

    #[test]
    fn test_resolve_document_path_denies_an_absolute_path_outside_the_sandbox() {
        let repo = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        let secret = outside.path().join("private-finances.csv");
        std::fs::write(
            &secret,
            "acct,balance
123,999
",
        )
        .unwrap();
        let ctx = doc_ctx(repo.path());

        let err = resolve_document_path(&secret.display().to_string(), &ctx).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside the session's read sandbox"),
            "{err}"
        );
    }

    #[test]
    fn test_resolve_document_path_denies_a_relative_traversal_outside_the_sandbox() {
        let base = tempfile::TempDir::new().unwrap();
        let repo = base.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let secret = base.path().join("secret.csv");
        std::fs::write(
            &secret, "a,b
1,2
",
        )
        .unwrap();
        let ctx = doc_ctx(&repo);

        let err = resolve_document_path("../secret.csv", &ctx).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside the session's read sandbox"),
            "{err}"
        );
    }

    #[test]
    fn test_resolve_document_path_allows_a_path_granted_via_allow() {
        let repo = tempfile::TempDir::new().unwrap();
        let granted = tempfile::TempDir::new().unwrap();
        let doc = granted.path().join("granted.csv");
        std::fs::write(
            &doc, "a,b
1,2
",
        )
        .unwrap();
        let ctx = ReplContext {
            repo_root: repo.path().to_path_buf(),
            allowed_read_roots: vec![granted.path().to_path_buf()],
        };

        let resolved = resolve_document_path(&doc.display().to_string(), &ctx).unwrap();
        assert_eq!(resolved, doc);
    }

    #[test]
    fn test_resolve_document_path_refuses_files_over_the_size_cap() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("big.csv"), "a,b\n").unwrap();
        let ctx = doc_ctx(dir.path());
        let err = resolve_document_path_with_cap("big.csv", &ctx, 1).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.starts_with("document_read: 'big.csv' is 4 bytes"),
            "{msg}"
        );
        assert!(msg.contains("1-byte limit"), "{msg}");
        assert!(resolve_document_path_with_cap("big.csv", &ctx, 4).is_ok());
    }

    #[test]
    fn test_check_extracted_size_refuses_oversized_text() {
        assert!(check_extracted_size("abcdef", "a.csv", 1_000).is_ok());
        let err = check_extracted_size("abcdef", "a.csv", 5).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.starts_with("document_read: 'a.csv' extracted to"),
            "{msg}"
        );
        assert!(msg.contains("over the 5 character limit"), "{msg}");
    }

    /// Writes a deflated zip with the given (name, bytes) entries.
    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        use std::io::Write as _;
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn test_preflight_container_passes_non_zip_formats_and_rejects_bad_zips() {
        let dir = tempfile::TempDir::new().unwrap();
        let csv = dir.path().join("a.csv");
        std::fs::write(&csv, "a,b\n").unwrap();
        assert!(preflight_container(&csv, "a.csv").is_ok());

        let bad = dir.path().join("bad.docx");
        std::fs::write(&bad, "not a zip").unwrap();
        let err = preflight_container(&bad, "bad.docx").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.starts_with("document_read: 'bad.docx' could not be parsed"),
            "{msg}"
        );
        assert!(msg.contains("docx container"), "{msg}");

        let missing = dir.path().join("missing.xlsx");
        let err = preflight_container(&missing, "missing.xlsx").unwrap_err();
        assert!(
            format!("{err:#}").contains("could not be opened"),
            "{err:#}"
        );
    }

    #[test]
    fn test_preflight_measures_inflated_size_not_archive_size() {
        // 200 KiB of zeros deflates to a few hundred bytes on disk; the
        // guard must count what the entry inflates to.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bomb.docx");
        let zeros = vec![0u8; 200 * 1024];
        write_zip(&path, &[("word/document.xml", &zeros)]);
        assert!(std::fs::metadata(&path).unwrap().len() < 10 * 1024);

        let err = preflight_container_with_limit(&path, "bomb.docx", 100 * 1024).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.starts_with("document_read: 'bomb.docx' inflates to more than 102400 bytes"),
            "{msg}"
        );
        assert!(preflight_container_with_limit(&path, "bomb.docx", 300 * 1024).is_ok());
    }

    #[test]
    fn test_preflight_counts_entries_cumulatively() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("many.xlsx");
        let chunk = vec![b'x'; 40 * 1024];
        write_zip(
            &path,
            &[
                ("xl/workbook.xml", &chunk),
                ("xl/worksheets/sheet1.xml", &chunk),
                ("xl/worksheets/sheet2.xml", &chunk),
            ],
        );
        // Each entry fits under 100 KiB alone; together they do not.
        let err = preflight_container_with_limit(&path, "many.xlsx", 100 * 1024).unwrap_err();
        assert!(err.to_string().contains("inflates to more than"), "{err}");
        assert!(preflight_container_with_limit(&path, "many.xlsx", 130 * 1024).is_ok());
    }

    #[test]
    fn round_trip_document_window_and_request() {
        let window = DocumentWindow {
            path: "a.xlsx".into(),
            format: "xlsx".into(),
            document_type: "excel".into(),
            size_bytes: 10,
            sections: vec![
                DocumentSection {
                    index: 0,
                    kind: "sheet".into(),
                    name: Some("Budget".into()),
                    offset: Some(22),
                    chars: 3,
                },
                DocumentSection {
                    index: 1,
                    kind: "paragraph".into(),
                    name: None,
                    offset: None,
                    chars: 1,
                },
            ],
            section: Some("Budget".into()),
            total_chars: 3,
            offset: 0,
            returned_chars: 3,
            truncated: false,
            next_offset: None,
            content: Some("a\tb".into()),
        };
        let json = serde_json::to_string(&window).unwrap();
        let recovered: DocumentWindow = serde_json::from_str(&json).unwrap();
        assert_eq!(window, recovered);
        assert!(
            !json.contains("next_offset"),
            "None fields are omitted: {json}"
        );

        let request = parse_request(&json!({"path": "a.csv", "offset": 3})).unwrap();
        let json = serde_json::to_string(&request).unwrap();
        let recovered: DocumentReadRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, recovered);
    }

    #[test]
    fn test_render_shows_sections_with_offsets_and_a_continuation_hint() {
        let mut window = window_for(
            vec![
                DocumentSection {
                    index: 0,
                    kind: "sheet".into(),
                    name: Some("Budget".into()),
                    offset: Some(22),
                    chars: 30,
                },
                DocumentSection {
                    index: 0,
                    kind: "csv".into(),
                    name: None,
                    offset: None,
                    chars: 5,
                },
            ],
            Some("0123456789"),
        );
        window.truncated = true;
        window.next_offset = Some(10);
        let text = render(&window);
        assert!(
            text.contains("[0] sheet \"Budget\" offset=22 30 chars"),
            "{text}"
        );
        assert!(text.contains("[0] csv 5 chars"), "{text}");
        assert!(text.contains("empty worksheets are omitted"), "{text}");
        assert!(
            text.contains("20 chars remain (about 2 more call(s)"),
            "{text}"
        );
        assert!(text.contains("continue with offset=10"), "{text}");
        assert!(text.contains("raise max_chars up to 32000"), "{text}");
        assert!(
            text.contains("[window ends mid-line; the next call continues from offset=10]"),
            "{text}"
        );

        let outline = DocumentWindow {
            content: None,
            returned_chars: 0,
            truncated: false,
            next_offset: None,
            ..window
        };
        let text = render(&outline);
        assert!(text.contains("outline only"), "{text}");
        assert!(!text.contains("0123456789"));
    }

    #[test]
    fn test_render_omits_the_table_on_continuation_pages_and_marks_completion() {
        let mut window = window_for(
            vec![DocumentSection {
                index: 0,
                kind: "sheet".into(),
                name: Some("Budget".into()),
                offset: Some(22),
                chars: 30,
            }],
            Some("tail\n"),
        );
        window.offset = 25;
        let text = render(&window);
        assert!(
            !text.contains("[0] sheet"),
            "continuation pages skip the table: {text}"
        );
        assert!(text.contains("chars 25..30 of 30; complete"), "{text}");
        assert!(text.ends_with("tail\n"), "{text}");
        assert!(!text.contains("mid-line"));
    }

    #[test]
    fn test_render_bounds_the_section_table() {
        let sections: Vec<DocumentSection> = (0..500)
            .map(|i| DocumentSection {
                index: i,
                kind: "paragraph".into(),
                name: None,
                offset: Some(i * 10),
                chars: 10,
            })
            .collect();
        let mut window = window_for(sections, Some("x"));
        window.format = "docx".into();
        let text = render(&window);
        assert_eq!(text.matches("] paragraph ").count(), MAX_RENDERED_SECTIONS);
        assert!(text.contains("468 more section(s) not listed"), "{text}");
        assert!(text.len() < 3_000, "{}", text.len());
        assert!(!text.contains("empty worksheets"));
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
                .starts_with("document_read: 'b.docx' extracted to more than 10 characters"),
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

    mod fixtures {
        use super::*;

        fn ctx_in(dir: &tempfile::TempDir) -> ReplContext {
            ReplContext {
                repo_root: dir.path().to_path_buf(),
                ..ReplContext::default()
            }
        }

        fn write_workbook(dir: &tempfile::TempDir) -> PathBuf {
            let path = dir.path().join("ledger.xlsx");
            let path_str = path.to_str().unwrap().to_string();
            let mut workbook = rust_xlsxwriter::Workbook::new(&path_str);
            let budget = workbook.add_worksheet();
            budget.set_name("Budget").unwrap();
            budget.write_string_only(0, 0, "Item").unwrap();
            budget.write_string_only(0, 1, "Amount").unwrap();
            budget.write_string_only(1, 0, "Rent").unwrap();
            budget.write_number_only(1, 1, 1800.0).unwrap();
            // A multi-line cell that looks like a sheet header is just text.
            budget
                .write_string_only(2, 0, "Note:\n=== Sheet: Evil ===\nend")
                .unwrap();
            let empty = workbook.add_worksheet();
            empty.set_name("Scratch").unwrap();
            let notes = workbook.add_worksheet();
            notes.set_name("Notes").unwrap();
            notes.write_string_only(0, 0, "Paid on the first").unwrap();
            workbook.close().unwrap();
            path
        }

        fn write_docx(dir: &tempfile::TempDir) -> PathBuf {
            use docx::{Docx, Paragraph, Run};
            let path = dir.path().join("letter.docx");
            let file = std::fs::File::create(&path).unwrap();
            Docx::new()
                .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Dear tenant,")))
                .add_paragraph(
                    Paragraph::new().add_run(Run::new().add_text("Your lease renews in March.")),
                )
                .build()
                .pack(file)
                .unwrap();
            path
        }

        #[tokio::test]
        async fn test_run_reads_csv_with_sections_and_complete_window() {
            let dir = tempfile::TempDir::new().unwrap();
            std::fs::write(
                dir.path().join("expenses.csv"),
                "date,vendor,amount\n2026-08-01,Grocer,42.10\n",
            )
            .unwrap();

            let outcome = DocumentReadTool
                .run(json!({"path": "expenses.csv"}), &ctx_in(&dir))
                .await
                .unwrap();

            assert!(outcome.ok);
            assert_eq!(outcome.payload["format"], "csv");
            assert_eq!(outcome.payload["truncated"], false);
            assert_eq!(outcome.payload["sections"][0]["kind"], "csv");
            assert_eq!(outcome.payload["sections"][0]["offset"], 0);
            assert!(outcome.payload["content"]
                .as_str()
                .unwrap()
                .contains("Grocer"));
            assert!(outcome.rendered.contains("expenses.csv"));
            assert!(outcome.rendered.contains("Grocer"));
            assert!(outcome.rendered.contains("complete"));
        }

        #[tokio::test]
        async fn test_run_pages_through_csv_on_line_boundaries() {
            let dir = tempfile::TempDir::new().unwrap();
            let body = "a,b\n1,2\n3,4\n";
            std::fs::write(dir.path().join("t.csv"), body).unwrap();
            let ctx = ctx_in(&dir);

            let mut offset = 0;
            let mut assembled = String::new();
            let mut calls = 0;
            loop {
                let outcome = DocumentReadTool
                    .run(
                        json!({"path": "t.csv", "offset": offset, "max_chars": 5}),
                        &ctx,
                    )
                    .await
                    .unwrap();
                calls += 1;
                let page = outcome.payload["content"].as_str().unwrap();
                assert!(
                    page.ends_with('\n'),
                    "pages end on line boundaries: {page:?}"
                );
                assembled.push_str(page);
                if outcome.payload["truncated"] == false {
                    break;
                }
                assert!(
                    outcome.rendered.contains("truncated"),
                    "{}",
                    outcome.rendered
                );
                offset = outcome.payload["next_offset"].as_u64().unwrap() as usize;
            }
            assert_eq!(assembled, body);
            assert_eq!(calls, 3);
        }

        #[tokio::test]
        async fn test_run_outline_omits_content() {
            let dir = tempfile::TempDir::new().unwrap();
            std::fs::write(dir.path().join("t.csv"), "a,b\n").unwrap();

            let outcome = DocumentReadTool
                .run(json!({"path": "t.csv", "outline": true}), &ctx_in(&dir))
                .await
                .unwrap();

            assert!(outcome.payload.get("content").is_none());
            assert_eq!(outcome.payload["total_chars"], 4);
            assert!(outcome.rendered.contains("outline only"));
        }

        #[tokio::test]
        async fn test_run_reads_xlsx_sheets_selects_one_and_treats_header_text_as_text() {
            let dir = tempfile::TempDir::new().unwrap();
            let path = write_workbook(&dir);
            let ctx = ctx_in(&dir);

            let all = DocumentReadTool
                .run(json!({"path": path.to_str().unwrap()}), &ctx)
                .await
                .unwrap();
            let sections = all.payload["sections"].as_array().unwrap();
            let names: Vec<&str> = sections
                .iter()
                .map(|s| s["name"].as_str().unwrap())
                .collect();
            // The empty "Scratch" sheet is omitted; the header-shaped cell
            // is ordinary text; indexes are workbook positions.
            assert_eq!(names, ["Budget", "Notes"]);
            assert_eq!(sections[1]["index"], 2);
            let content = all.payload["content"].as_str().unwrap();
            assert!(content.contains("Rent\t1800"), "{content}");
            assert!(content.contains("=== Sheet: Evil ==="), "{content}");
            assert!(content.contains("Paid on the first"));
            // Section offsets land on each sheet's body in the paged text.
            for section in sections {
                let offset = section["offset"].as_u64().unwrap() as usize;
                let chars = section["chars"].as_u64().unwrap() as usize;
                let body: String = content.chars().skip(offset).take(chars).collect();
                assert!(!body.starts_with("=== Sheet"), "{body}");
                assert!(!body.is_empty());
            }
            assert!(all.rendered.contains("empty worksheets are omitted"));

            let notes = DocumentReadTool
                .run(json!({"path": "ledger.xlsx", "sheet": "notes"}), &ctx)
                .await
                .unwrap();
            assert_eq!(notes.payload["section"], "Notes");
            let content = notes.payload["content"].as_str().unwrap();
            assert_eq!(content, "Paid on the first");

            let err = DocumentReadTool
                .run(json!({"path": "ledger.xlsx", "sheet": "Evil"}), &ctx)
                .await
                .unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("Budget, Notes") && msg.contains("empty worksheets are omitted"),
                "{msg}"
            );
        }

        #[tokio::test]
        async fn test_run_handles_corner_cells_without_densifying_the_grid() {
            // The dense-grid parser would attempt a 16384 x 1048576 vector
            // for this sheet and abort the process; streaming renders two
            // cells and two gap markers.
            let dir = tempfile::TempDir::new().unwrap();
            let path = dir.path().join("corners.xlsx");
            let path_str = path.to_str().unwrap().to_string();
            let mut workbook = rust_xlsxwriter::Workbook::new(&path_str);
            let sheet = workbook.add_worksheet();
            sheet.set_name("Wide").unwrap();
            sheet.write_string_only(0, 0, "start").unwrap();
            sheet.write_string_only(1_048_575, 16_383, "end").unwrap();
            workbook.close().unwrap();

            let outcome = DocumentReadTool
                .run(json!({"path": "corners.xlsx"}), &ctx_in(&dir))
                .await
                .unwrap();

            let content = outcome.payload["content"].as_str().unwrap();
            assert!(
                content.contains("start\n[1048574 empty rows]\n[16383 empty columns]\tend"),
                "{content}"
            );
            assert!(outcome.payload["total_chars"].as_u64().unwrap() < 200);
        }

        #[test]
        fn test_extract_workbook_enforces_text_and_cell_budgets() {
            // 200 cells referencing one 500-character shared string would
            // materialize 100 KB under the dense parser; the streamed
            // extractor stops at the budget instead.
            let dir = tempfile::TempDir::new().unwrap();
            let path = dir.path().join("amplify.xlsx");
            let path_str = path.to_str().unwrap().to_string();
            let mut workbook = rust_xlsxwriter::Workbook::new(&path_str);
            let sheet = workbook.add_worksheet();
            let long = "x".repeat(500);
            for row in 0..200u32 {
                sheet.write_string_only(row, 0, &long).unwrap();
            }
            workbook.close().unwrap();

            let err = extract_workbook(
                &path,
                "amplify.xlsx",
                ExtractBudget {
                    max_chars: 10_000,
                    max_cells: MAX_CELLS,
                },
            )
            .unwrap_err();
            assert!(
                err.to_string()
                    .contains("extracted to more than 10000 characters"),
                "{err}"
            );

            let err = extract_workbook(
                &path,
                "amplify.xlsx",
                ExtractBudget {
                    max_chars: MAX_EXTRACTED_CHARS,
                    max_cells: 50,
                },
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("more than 50 non-empty cells"),
                "{err}"
            );

            let parsed = extract_workbook(&path, "amplify.xlsx", ExtractBudget::DEFAULT).unwrap();
            assert_eq!(parsed.sheets.len(), 1);
            assert_eq!(parsed.sections[0].chars, 200 * 500 + 199);
        }

        #[tokio::test]
        async fn test_run_reads_docx_paragraphs_with_offsets() {
            let dir = tempfile::TempDir::new().unwrap();
            write_docx(&dir);

            let outcome = DocumentReadTool
                .run(json!({"path": "letter.docx"}), &ctx_in(&dir))
                .await
                .unwrap();

            assert_eq!(outcome.payload["format"], "docx");
            assert_eq!(outcome.payload["document_type"], "word");
            assert_eq!(outcome.payload["sections"][0]["kind"], "paragraph");
            assert_eq!(outcome.payload["sections"][0]["offset"], 0);
            assert_eq!(
                outcome.payload["sections"][0]["chars"],
                outcome.payload["total_chars"]
            );
            let content = outcome.payload["content"].as_str().unwrap();
            assert!(content.contains("Dear tenant"), "{content}");
            assert!(content.contains("renews in March"), "{content}");
        }

        #[tokio::test]
        async fn test_run_surfaces_argument_path_and_parser_errors() {
            let dir = tempfile::TempDir::new().unwrap();
            let ctx = ctx_in(&dir);
            let err = DocumentReadTool.run(json!({}), &ctx).await.unwrap_err();
            assert!(err.to_string().contains("'path'"), "{err}");

            let err = DocumentReadTool
                .run(json!({"path": "nope.docx"}), &ctx)
                .await
                .unwrap_err();
            assert!(
                format!("{err:#}").starts_with("document_read: 'nope.docx' not found"),
                "{err:#}"
            );

            std::fs::write(dir.path().join("bad.xlsx"), "not a workbook").unwrap();
            let err = DocumentReadTool
                .run(json!({"path": "bad.xlsx"}), &ctx)
                .await
                .unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.starts_with("document_read: 'bad.xlsx' could not be parsed"),
                "{msg}"
            );

            std::fs::write(dir.path().join("bad.csv"), [0xff, 0xfe, b'a']).unwrap();
            let err = DocumentReadTool
                .run(json!({"path": "bad.csv"}), &ctx)
                .await
                .unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.starts_with("document_read: 'bad.csv' could not be parsed"),
                "{msg}"
            );
        }

        #[test]
        fn test_preflight_container_accepts_real_documents_and_measures_inflation() {
            let dir = tempfile::TempDir::new().unwrap();
            let docx = write_docx(&dir);
            let xlsx = write_workbook(&dir);
            for (path, raw) in [(&docx, "letter.docx"), (&xlsx, "ledger.xlsx")] {
                assert!(preflight_container(path, raw).is_ok(), "{raw}");
                let err = preflight_container_with_limit(path, raw, 16).unwrap_err();
                let msg = err.to_string();
                assert!(
                    msg.starts_with(&format!(
                        "document_read: '{raw}' inflates to more than 16 bytes"
                    )),
                    "{msg}"
                );
            }
        }

        /// The two package parts every docx carries, plus a
        /// `word/document.xml` built from `document_xml` verbatim. Written
        /// by hand rather than through the docx builder so a fixture can
        /// hold revision marks, field codes, tables, or malformed XML that
        /// the builder cannot produce.
        fn write_docx_raw(dir: &tempfile::TempDir, name: &str, document_xml: &[u8]) -> PathBuf {
            const CONTENT_TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
            const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
            let path = dir.path().join(name);
            write_zip(
                &path,
                &[
                    ("[Content_Types].xml", CONTENT_TYPES),
                    ("_rels/.rels", RELS),
                    ("word/document.xml", document_xml),
                ],
            );
            path
        }

        /// A docx whose body is `body`, wrapped in the namespaces Word
        /// declares on every document.
        fn write_docx_body(dir: &tempfile::TempDir, name: &str, body: &str) -> PathBuf {
            let document_xml = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>{body}</w:body></w:document>"#
            );
            write_docx_raw(dir, name, document_xml.as_bytes())
        }

        /// A workbook with one real worksheet and one chart sheet, hand
        /// built because the workbook writer in the dependency tree cannot
        /// emit a chart sheet. calamine decides a sheet's type from its
        /// part path, so the chart sheet must live under `xl/chartsheets/`
        /// and be reached through the workbook relationships.
        fn write_chartsheet_workbook(dir: &tempfile::TempDir, name: &str) -> PathBuf {
            const CONTENT_TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/chartsheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml"/></Types>"#;
            const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
            const WORKBOOK: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="Revenue chart" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
            const WORKBOOK_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet" Target="chartsheets/sheet1.xml"/></Relationships>"#;
            const WORKSHEET: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:B2"/><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Quarter</t></is></c><c r="B1" t="inlineStr"><is><t>Revenue</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Q1</t></is></c><c r="B2"><v>1200</v></c></row></sheetData></worksheet>"#;
            const CHARTSHEET: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<chartsheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetPr/><sheetViews><sheetView workbookViewId="0" zoomScale="100"/></sheetViews><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><drawing r:id="rId1"/></chartsheet>"#;
            let path = dir.path().join(name);
            write_zip(
                &path,
                &[
                    ("[Content_Types].xml", CONTENT_TYPES),
                    ("_rels/.rels", RELS),
                    ("xl/workbook.xml", WORKBOOK),
                    ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
                    ("xl/worksheets/sheet1.xml", WORKSHEET),
                    ("xl/chartsheets/sheet1.xml", CHARTSHEET),
                ],
            );
            path
        }

        #[tokio::test]
        async fn test_run_reads_a_workbook_with_a_chart_sheet_instead_of_failing() {
            let dir = tempfile::TempDir::new().unwrap();
            write_chartsheet_workbook(&dir, "revenue.xlsx");
            let ctx = ctx_in(&dir);

            let outcome = DocumentReadTool
                .run(json!({"path": "revenue.xlsx"}), &ctx)
                .await
                .unwrap();

            assert!(outcome.ok);
            let content = outcome.payload["content"].as_str().unwrap();
            assert!(content.contains("Quarter\tRevenue"), "{content}");
            assert!(content.contains("Q1\t1200"), "{content}");
            // The chart sheet holds no cells, so it is skipped exactly as an
            // empty worksheet is: one section, and the workbook still reads.
            let sections = outcome.payload["sections"].as_array().unwrap();
            assert_eq!(sections.len(), 1, "{sections:?}");
            assert_eq!(sections[0]["name"], "Data");
            assert!(!content.contains("Revenue chart"), "{content}");

            // Asking for it by name says so truthfully rather than
            // pretending the sheet does not exist in the workbook.
            let err = DocumentReadTool
                .run(
                    json!({"path": "revenue.xlsx", "sheet": "Revenue chart"}),
                    &ctx,
                )
                .await
                .unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("not found among readable worksheets"), "{msg}");
            assert!(msg.contains("available: Data"), "{msg}");
        }

        #[tokio::test]
        async fn test_run_reads_a_docx_of_mostly_empty_paragraphs_within_budget() {
            let dir = tempfile::TempDir::new().unwrap();
            let mut body = String::new();
            for _ in 0..25_000 {
                body.push_str("<w:p/>");
                body.push_str("<w:p><w:r><w:t xml:space=\"preserve\">   </w:t></w:r></w:p>");
            }
            body.push_str("<w:p><w:r><w:t>The tail that matters.</w:t></w:r></w:p>");
            body.push_str(
                "<w:p><w:r><w:t>Signed,</w:t><w:tab/><w:t>the landlord</w:t></w:r></w:p>",
            );
            let path = write_docx_body(&dir, "many.docx", &body);
            let on_disk = std::fs::metadata(&path).unwrap().len();

            let outcome = DocumentReadTool
                .run(json!({"path": "many.docx"}), &ctx_in(&dir))
                .await
                .unwrap();

            // 50,000 blank paragraphs cost nothing: the text, the section
            // table, and the reported window are all the size of the tail.
            let content = outcome.payload["content"].as_str().unwrap();
            assert_eq!(content, "The tail that matters.\nSigned,\tthe landlord\n");
            assert_eq!(outcome.payload["truncated"], false);
            assert_eq!(
                outcome.payload["total_chars"].as_u64().unwrap(),
                content.chars().count() as u64
            );
            assert_eq!(outcome.payload["sections"].as_array().unwrap().len(), 1);
            // The reported size is the file on disk, itself under the source
            // cap, not the inflated XML.
            assert_eq!(outcome.payload["size_bytes"].as_u64().unwrap(), on_disk);
            assert!(on_disk < MAX_DOCUMENT_BYTES, "{on_disk}");
        }

        #[tokio::test]
        async fn test_run_excludes_deleted_moved_and_field_code_text_from_a_docx() {
            let dir = tempfile::TempDir::new().unwrap();
            let body = concat!(
                // A deletion written the ordinary way, with `w:delText`.
                "<w:p><w:r><w:t>Kept before.</w:t></w:r>",
                "<w:del w:id=\"1\" w:author=\"a\"><w:r><w:delText>DELETEDTEXT</w:delText></w:r></w:del>",
                "<w:r><w:t> Kept after.</w:t></w:r></w:p>",
                // A deletion whose run still carries `w:t`: only skipping the
                // whole `w:del` subtree keeps this out.
                "<w:p><w:del w:id=\"2\"><w:r><w:t>DELETEDRUN</w:t></w:r></w:del>",
                "<w:r><w:t>Visible tail.</w:t></w:r></w:p>",
                // A field: the instruction is a directive, the result is what
                // a reader sees.
                "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>",
                "<w:r><w:instrText xml:space=\"preserve\"> MERGEFIELD SECRETCODE </w:instrText></w:r>",
                "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>",
                "<w:r><w:t>Field result.</w:t></w:r>",
                "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
                // A tracked move: keep the destination, drop the source copy.
                "<w:p><w:moveFrom w:id=\"3\"><w:r><w:t>MOVEDAWAY</w:t></w:r></w:moveFrom>",
                "<w:moveTo w:id=\"4\"><w:r><w:t>Moved here.</w:t></w:r></w:moveTo></w:p>",
            );
            write_docx_body(&dir, "tracked.docx", body);

            let outcome = DocumentReadTool
                .run(json!({"path": "tracked.docx"}), &ctx_in(&dir))
                .await
                .unwrap();

            let content = outcome.payload["content"].as_str().unwrap();
            assert_eq!(
                content,
                "Kept before. Kept after.\nVisible tail.\nField result.\nMoved here.\n",
            );
            for absent in [
                "DELETEDTEXT",
                "DELETEDRUN",
                "MERGEFIELD",
                "SECRETCODE",
                "MOVEDAWAY",
            ] {
                assert!(!content.contains(absent), "{absent} in {content}");
            }
        }

        #[test]
        fn test_extract_word_renders_table_rows_with_tab_separated_cells() {
            let dir = tempfile::TempDir::new().unwrap();
            let body = concat!(
                "<w:p><w:r><w:t>Before the table.</w:t></w:r></w:p>",
                "<w:tbl>",
                "<w:tr><w:tc><w:p><w:r><w:t>Item</w:t></w:r></w:p></w:tc>",
                "<w:tc><w:p><w:r><w:t>Amount</w:t></w:r></w:p></w:tc></w:tr>",
                // Two paragraphs in one cell join with a space, and a tab
                // inside a cell cannot forge a column break.
                "<w:tr><w:tc><w:p><w:r><w:t>Rent</w:t></w:r></w:p>",
                "<w:p><w:r><w:t>monthly</w:t><w:tab/><w:t>due</w:t></w:r></w:p></w:tc>",
                "<w:tc><w:p><w:r><w:t>1800</w:t></w:r></w:p></w:tc></w:tr>",
                "</w:tbl>",
                "<w:p><w:r><w:t>After the table.</w:t></w:r></w:p>",
            );
            let path = write_docx_body(&dir, "table.docx", body);

            let parsed = extract_word(&path, "table.docx", ExtractBudget::DEFAULT).unwrap();

            assert_eq!(
                parsed.text,
                "Before the table.\nItem\tAmount\nRent monthly due\t1800\nAfter the table.\n",
            );
            assert_eq!(parsed.format, "docx");
            assert_eq!(parsed.document_type, "word");
            assert!(parsed.sheets.is_empty());
        }

        #[test]
        fn test_extract_word_empty_body_yields_no_text_and_no_sections() {
            let dir = tempfile::TempDir::new().unwrap();
            let path = write_docx_body(&dir, "blank.docx", "");

            let parsed = extract_word(&path, "blank.docx", ExtractBudget::DEFAULT).unwrap();

            assert!(parsed.text.is_empty(), "{:?}", parsed.text);
            assert!(parsed.sections.is_empty(), "{:?}", parsed.sections);
            assert!(parsed.size_bytes > 0);
        }

        #[test]
        fn test_extract_word_refuses_one_oversized_paragraph() {
            let dir = tempfile::TempDir::new().unwrap();
            // One paragraph split across many runs: the budget is enforced
            // as the paragraph buffer grows, not only when it is flushed.
            let mut body = String::from("<w:p>");
            for _ in 0..500 {
                body.push_str("<w:r><w:t>0123456789</w:t></w:r>");
            }
            body.push_str("</w:p>");
            let path = write_docx_body(&dir, "long.docx", &body);

            let budget = ExtractBudget {
                max_chars: 64,
                max_cells: MAX_CELLS,
            };
            let err = extract_word(&path, "long.docx", budget).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.starts_with("document_read: 'long.docx' extracted to more than 64 characters"),
                "{msg}"
            );
            // The same document is fine when the budget allows it.
            let parsed = extract_word(&path, "long.docx", ExtractBudget::DEFAULT).unwrap();
            assert_eq!(parsed.text.chars().count(), 500 * 10 + 1);
        }

        #[test]
        fn test_extract_word_malformed_xml_returns_an_error_not_a_panic() {
            let dir = tempfile::TempDir::new().unwrap();
            let path = write_docx_raw(
                &dir,
                "broken.docx",
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>half a sentence</w:x></w:r></w:p></w:body></w:document>"#,
            );

            let err = extract_word(&path, "broken.docx", ExtractBudget::DEFAULT).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.starts_with(
                    "document_read: 'broken.docx' could not be parsed: word/document.xml is malformed"
                ),
                "{msg}"
            );

            // A container with no `word/document.xml` at all is also typed.
            let empty = dir.path().join("nodoc.docx");
            write_zip(&empty, &[("[Content_Types].xml", b"<Types/>")]);
            let err = extract_word(&empty, "nodoc.docx", ExtractBudget::DEFAULT).unwrap_err();
            assert!(err.to_string().contains("no word/document.xml"), "{err}");
        }

        #[tokio::test]
        async fn test_run_refuses_a_docx_whose_part_inflates_past_the_container_cap() {
            let dir = tempfile::TempDir::new().unwrap();
            // Highly compressible, so the file on disk stays far under the
            // source cap while `word/document.xml` alone inflates past the
            // 64 MiB container cap.
            let filler = "<w:p><w:pPr><w:spacing w:before=\"120\" w:after=\"120\"/></w:pPr></w:p>";
            let repeats = (MAX_DECOMPRESSED_BYTES as usize / filler.len()) + 4096;
            let path = write_docx_body(&dir, "bomb.docx", &filler.repeat(repeats));
            let on_disk = std::fs::metadata(&path).unwrap().len();
            assert!(on_disk < MAX_DOCUMENT_BYTES, "{on_disk}");

            let err = DocumentReadTool
                .run(json!({"path": "bomb.docx"}), &ctx_in(&dir))
                .await
                .unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.starts_with(&format!(
                    "document_read: 'bomb.docx' inflates to more than {MAX_DECOMPRESSED_BYTES} bytes"
                )),
                "{msg}"
            );

            // Reached directly, without the container preflight, the part is
            // refused rather than answered from whatever prefix fits under
            // the cap.
            let err = extract_word(&path, "bomb.docx", ExtractBudget::DEFAULT).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.starts_with(&format!(
                    "document_read: 'bomb.docx' has a word/document.xml that inflates to more \
                     than {MAX_DECOMPRESSED_BYTES} bytes"
                )),
                "{msg}"
            );
        }

        #[test]
        fn test_extract_word_cell_text_cannot_forge_a_column_or_row_break() {
            let dir = tempfile::TempDir::new().unwrap();
            let body = concat!(
                "<w:tbl><w:tr>",
                // A literal tab and a literal newline carried in the text.
                "<w:tc><w:p><w:r><w:t>a\tb\nc</w:t></w:r></w:p></w:tc>",
                // The same two characters written as numeric references.
                "<w:tc><w:p><w:r><w:t>d&#9;e&#10;f</w:t></w:r></w:p></w:tc>",
                // And again inside a CDATA section.
                "<w:tc><w:p><w:r><w:t><![CDATA[g\th\ni]]></w:t></w:r></w:p></w:tc>",
                "</w:tr></w:tbl>",
            );
            let path = write_docx_body(&dir, "forge.docx", body);

            let parsed = extract_word(&path, "forge.docx", ExtractBudget::DEFAULT).unwrap();

            // One row, three columns: every smuggled break became a space.
            assert_eq!(parsed.text, "a b c\td e f\tg h i\n");
            assert_eq!(parsed.text.matches('\n').count(), 1);
            assert_eq!(parsed.text.trim_end().matches('\t').count(), 2);
        }

        #[test]
        fn test_extract_word_paragraph_text_and_breaks_stay_on_one_line() {
            let dir = tempfile::TempDir::new().unwrap();
            let body = concat!(
                "<w:p><w:r><w:t>x&#10;y</w:t></w:r></w:p>",
                "<w:p><w:r><w:t>literal\nnewline</w:t></w:r></w:p>",
                // A structural break is a space too: one line is one paragraph.
                "<w:p><w:r><w:t>line one</w:t><w:br/><w:t>line two</w:t></w:r></w:p>",
                // A tab outside a table is ordinary text and cannot break a line.
                "<w:p><w:r><w:t>left</w:t><w:tab/><w:t>right</w:t></w:r></w:p>",
            );
            let path = write_docx_body(&dir, "breaks.docx", body);

            let parsed = extract_word(&path, "breaks.docx", ExtractBudget::DEFAULT).unwrap();

            assert_eq!(
                parsed.text,
                "x y\nliteral newline\nline one line two\nleft\tright\n",
            );
            // Four paragraphs in, four lines out.
            assert_eq!(parsed.text.matches('\n').count(), 4);
        }

        #[test]
        fn test_extract_word_keeps_outer_cell_text_around_a_nested_table() {
            let dir = tempfile::TempDir::new().unwrap();
            let body = concat!(
                "<w:tbl><w:tr>",
                "<w:tc><w:p><w:r><w:t>OUTER</w:t></w:r></w:p>",
                "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>INNER</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
                "</w:tc>",
                "<w:tc><w:p><w:r><w:t>RIGHT</w:t></w:r></w:p></w:tc>",
                "</w:tr></w:tbl>",
            );
            let path = write_docx_body(&dir, "nested.docx", body);

            let parsed = extract_word(&path, "nested.docx", ExtractBudget::DEFAULT).unwrap();

            // The containing cell keeps its own text, the nested table merges
            // into it, and the row still has exactly two columns.
            assert_eq!(parsed.text, "OUTER INNER\tRIGHT\n");
            assert_eq!(parsed.text.trim_end().matches('\t').count(), 1);
            assert!(parsed.text.contains("OUTER"), "{:?}", parsed.text);
        }

        #[test]
        fn test_extract_word_keeps_a_simple_field_result_but_not_its_instruction() {
            let dir = tempfile::TempDir::new().unwrap();
            // `w:fldSimple` carries its instruction in an attribute, and
            // attributes are never read, so only the cached result survives.
            let body = concat!(
                "<w:p><w:fldSimple w:instr=\" MERGEFIELD SECRETCODE \\* MERGEFORMAT \">",
                "<w:r><w:t>Cached result.</w:t></w:r></w:fldSimple></w:p>",
            );
            let path = write_docx_body(&dir, "field.docx", body);

            let parsed = extract_word(&path, "field.docx", ExtractBudget::DEFAULT).unwrap();

            assert_eq!(parsed.text, "Cached result.\n");
            assert!(!parsed.text.contains("SECRETCODE"), "{:?}", parsed.text);
        }

        #[test]
        fn test_extract_word_finds_the_part_whatever_its_case() {
            let dir = tempfile::TempDir::new().unwrap();
            // OPC part names compare case-insensitively; calamine resolves
            // workbook parts the same way, so this must open too.
            let path = dir.path().join("cased.docx");
            write_zip(
                &path,
                &[(
                    "Word/Document.XML",
                    br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Found it.</w:t></w:r></w:p></w:body></w:document>"#,
                )],
            );

            let parsed = extract_word(&path, "cased.docx", ExtractBudget::DEFAULT).unwrap();

            assert_eq!(parsed.text, "Found it.\n");
        }

        #[test]
        fn test_extract_word_rejects_a_cdata_section_that_is_not_utf8() {
            let dir = tempfile::TempDir::new().unwrap();
            let mut document_xml = Vec::new();
            document_xml.extend_from_slice(
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t><![CDATA["#,
            );
            document_xml.extend_from_slice(&[0xff, 0xfe, 0x00]);
            document_xml.extend_from_slice(br#"]]></w:t></w:r></w:p></w:body></w:document>"#);
            let path = write_docx_raw(&dir, "cdata.docx", &document_xml);

            let err = extract_word(&path, "cdata.docx", ExtractBudget::DEFAULT).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.starts_with(
                    "document_read: 'cdata.docx' could not be parsed: a CDATA section is not \
                     valid UTF-8"
                ),
                "{msg}"
            );
        }
    }
}
