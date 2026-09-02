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
//!   to one huge shared string cost only what they render;
//! - `csv` and `docx` text is checked against [`MAX_EXTRACTED_CHARS`] after
//!   parsing, where the parsers' own memory is already bounded by the file
//!   and inflation caps;
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
                 offset=N'. Use sheet to read one worksheet (empty worksheets are omitted), or \
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
                        "description": "Worksheet name to read (spreadsheets only, case-insensitive; empty worksheets are omitted). When set, offset is relative to that sheet's text."
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
        let path = resolve_document_path(&request.path, &ctx.repo_root)?;
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

/// Resolves `raw` against `repo_root` when relative and checks it names an
/// existing regular file in an accepted format under [`MAX_DOCUMENT_BYTES`].
/// Error messages lead with the path the caller supplied, so two different
/// bad paths never share an error signature.
pub fn resolve_document_path(raw: &str, repo_root: &Path) -> Result<PathBuf> {
    resolve_document_path_with_cap(raw, repo_root, MAX_DOCUMENT_BYTES)
}

/// [`resolve_document_path`] with an explicit size cap; the test seam.
pub fn resolve_document_path_with_cap(
    raw: &str,
    repo_root: &Path,
    max_bytes: u64,
) -> Result<PathBuf> {
    let candidate = PathBuf::from(raw);
    let path = if candidate.is_absolute() || repo_root.as_os_str().is_empty() {
        candidate
    } else {
        repo_root.join(candidate)
    };
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
/// cell by cell by this module, `csv` and `docx` go through the `office`
/// parsers and are size-checked afterwards.
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
        "csv" | "docx" => {
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
        let mut reader = workbook.worksheet_cells_reader(name).map_err(|e| {
            anyhow::anyhow!("document_read: '{raw}' could not be parsed: sheet '{name}': {e}")
        })?;
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

    /// Appends one cell at zero-based (`row`, `col`); cells must arrive in
    /// row-major order, as the cell reader yields them.
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

/// Adapts an `office` parser result (`csv`, `docx`) into a
/// [`ParsedDocument`], computing section offsets from the parsers' fixed
/// layouts: one span for CSV, and for Word one paragraph per line grouped
/// into chunks joined by a blank line. Offsets are dropped when a layout
/// cannot be matched exactly.
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
                "no readable worksheets in this xlsx document: every sheet is empty (empty \
                 worksheets are omitted)"
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
                "sheet '{wanted}' not found among non-empty sheets (empty worksheets are \
                 omitted); available: {}",
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
                "  (empty worksheets are omitted; section indexes are workbook positions; \
                 section offsets are whole-document offsets)\n",
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

    #[test]
    fn test_resolve_document_path_joins_relative_paths_and_validates() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.csv"), "x,y\n").unwrap();
        let resolved = resolve_document_path("a.csv", dir.path()).unwrap();
        assert_eq!(resolved, dir.path().join("a.csv"));
        let absolute = dir.path().join("a.csv");
        assert_eq!(
            resolve_document_path(absolute.to_str().unwrap(), Path::new("/elsewhere")).unwrap(),
            absolute
        );

        let err = resolve_document_path("missing.csv", dir.path()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.starts_with("document_read: 'missing.csv' not found"),
            "{msg}"
        );

        std::fs::write(dir.path().join("notes.pdf"), "%PDF").unwrap();
        let err = resolve_document_path("notes.pdf", dir.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("document_read: 'notes.pdf'"), "{msg}");
        assert!(msg.contains("unsupported extension 'pdf'"), "{msg}");
        assert!(
            msg.contains("xlsx") && msg.contains("csv") && msg.contains("docx"),
            "{msg}"
        );

        std::fs::create_dir(dir.path().join("folder.csv")).unwrap();
        let err = resolve_document_path("folder.csv", dir.path()).unwrap_err();
        assert!(err.to_string().contains("is a directory"), "{err}");

        std::fs::write(dir.path().join("old.xls"), "binary").unwrap();
        let err = resolve_document_path("old.xls", dir.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.starts_with("document_read: 'old.xls' is a legacy .xls workbook"),
            "{msg}"
        );
        assert!(msg.contains("convert it to .xlsx"), "{msg}");
    }

    #[test]
    fn test_resolve_document_path_refuses_files_over_the_size_cap() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("big.csv"), "a,b\n").unwrap();
        let err = resolve_document_path_with_cap("big.csv", dir.path(), 1).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.starts_with("document_read: 'big.csv' is 4 bytes"),
            "{msg}"
        );
        assert!(msg.contains("1-byte limit"), "{msg}");
        assert!(resolve_document_path_with_cap("big.csv", dir.path(), 4).is_ok());
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
    }
}
