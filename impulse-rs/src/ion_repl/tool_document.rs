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
//! Bounds: the source file is refused above [`MAX_DOCUMENT_BYTES`]; the
//! content window is capped at [`MAX_CHARS_CAP`] characters; the rendered
//! section table is capped at [`MAX_RENDERED_SECTIONS`] rows and shown only
//! on the first page or in outline mode; parsing runs on the blocking pool
//! so the loop contract's wall clock can still fire.
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
/// Largest source file the tool will parse. Spreadsheets and Word files are
/// zip-compressed, so this bounds far more than 10 MiB of extracted text.
pub const MAX_DOCUMENT_BYTES: u64 = 10 * 1024 * 1024;
/// Most section rows rendered for the model; the payload keeps them all.
pub const MAX_RENDERED_SECTIONS: usize = 32;

const SHEET_HEADER_PREFIX: &str = "=== Sheet: ";
const SHEET_HEADER_SUFFIX: &str = " ===";
const SUPPORTED_FORMATS: &str = "xlsx, xls, csv, docx";

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
/// to the section. `offset` is absent when the parser's layout could not be
/// matched.
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

#[async_trait]
impl ReplTool for DocumentReadTool {
    fn name(&self) -> &'static str {
        "document_read"
    }

    fn usage(&self) -> &'static str {
        "document_read {\"path\": \"...\", \"sheet\": \"...\", \"outline\": false, \
         \"offset\": 0, \"max_chars\": 12000} -- read a spreadsheet or Word document \
         (xlsx/xls/csv/docx) as text, paged by character offset"
    }

    fn json_schema(&self) -> Value {
        json!({
            "name": "document_read",
            "description": format!(
                "Read a spreadsheet (xlsx, xls, csv) or Word document (docx) as plain text. \
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
        // Parsing inflates the whole document synchronously (calamine,
        // docx-rs). Run it off the async runtime so the loop contract's
        // wall-clock timeout can still fire while a large file parses.
        let window = {
            let request = request.clone();
            let path = path.clone();
            tokio::task::spawn_blocking(move || -> Result<DocumentWindow> {
                let parsed = office::parse_document(&path).map_err(|e| {
                    anyhow::anyhow!("document_read: '{}' could not be parsed: {e}", request.path)
                })?;
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
        Some(Value::String(_)) => None,
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
/// existing regular file in a readable format under [`MAX_DOCUMENT_BYTES`].
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

/// The addressable sections of a parsed document, with offsets in the
/// character coordinates of `parsed.content`.
///
/// Sheet names are recovered by walking the parser's fixed layout
/// (`=== Sheet: name ===\n` + body + `\n\n` per non-empty sheet) in lockstep
/// with the sheet chunks, so a cell whose text merely looks like a header
/// can never add, rename, or shift a section. Word sections are groups of
/// paragraphs the parser emitted one per line. When a layout cannot be
/// matched, the affected sections keep their chunk sizes but carry no offset.
pub fn sections_of(parsed: &ExtractionResult) -> Vec<DocumentSection> {
    let mut sections = Vec::with_capacity(parsed.chunks.len());
    let mut cursor = 0usize;
    let mut rest = parsed.content.as_str();
    let mut layout_ok = true;

    for chunk in &parsed.chunks {
        let chunk_chars = chunk.content.chars().count();
        match chunk.chunk_type.as_str() {
            "sheet" => {
                let matched = if layout_ok {
                    match_sheet_header(rest, &chunk.content)
                } else {
                    None
                };
                match matched {
                    Some((name, header_chars, tail)) => {
                        let offset = cursor + header_chars;
                        sections.push(DocumentSection {
                            index: chunk.index,
                            kind: chunk.chunk_type.clone(),
                            name: Some(name),
                            offset: Some(offset),
                            chars: chunk_chars,
                        });
                        let consumed = rest.chars().count() - tail.chars().count();
                        cursor += consumed;
                        rest = tail;
                    }
                    None => {
                        layout_ok = false;
                        sections.push(DocumentSection {
                            index: chunk.index,
                            kind: chunk.chunk_type.clone(),
                            name: None,
                            offset: None,
                            chars: chunk_chars,
                        });
                    }
                }
            }
            "csv" => {
                sections.push(DocumentSection {
                    index: chunk.index,
                    kind: chunk.chunk_type.clone(),
                    name: None,
                    offset: Some(0),
                    chars: parsed.content.chars().count(),
                });
            }
            "paragraph" => {
                // The parser wrote every paragraph followed by one newline,
                // and grouped them into chunks joined by a blank line.
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
                chars: chunk_chars,
            }),
        }
    }

    // Paragraph offsets are only trustworthy if the reconstruction spanned
    // the whole text exactly.
    let paragraph_layout_holds =
        sections.iter().all(|s| s.kind != "paragraph") || cursor == parsed.content.chars().count();
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
    sections
}

/// Matches one sheet header at the start of `rest` followed by exactly
/// `body`, returning the sheet name, the header's length in characters
/// (including its newline), and the text after the body's trailing blank
/// line.
fn match_sheet_header<'a>(rest: &'a str, body: &str) -> Option<(String, usize, &'a str)> {
    let after_prefix = rest.strip_prefix(SHEET_HEADER_PREFIX)?;
    let (header_rest, after_header) = after_prefix.split_once('\n')?;
    let name = header_rest.strip_suffix(SHEET_HEADER_SUFFIX)?;
    let after_body = after_header.strip_prefix(body)?;
    let tail = after_body.strip_prefix("\n\n").unwrap_or(after_body);
    let header_chars = SHEET_HEADER_PREFIX.chars().count() + header_rest.chars().count() + 1;
    Some((name.to_string(), header_chars, tail))
}

/// Sheet names in chunk order, from the same layout walk as
/// [`sections_of`].
pub fn sheet_names(parsed: &ExtractionResult) -> Vec<String> {
    sections_of(parsed)
        .into_iter()
        .filter(|s| s.kind == "sheet")
        .filter_map(|s| s.name)
        .collect()
}

/// The text a call reads from: the whole document, or one sheet when
/// `sheet` is given. Sheet selection is only meaningful for spreadsheets,
/// and only non-empty worksheets exist after parsing.
pub fn select_text(
    parsed: &ExtractionResult,
    sheet: Option<&str>,
) -> Result<(String, Option<String>)> {
    let Some(wanted) = sheet else {
        return Ok((parsed.content.clone(), None));
    };
    let is_workbook = matches!(parsed.metadata.format.as_str(), "xlsx" | "xls");
    let named: Vec<(usize, String)> = sections_of(parsed)
        .into_iter()
        .filter(|s| s.kind == "sheet")
        .enumerate()
        .filter_map(|(ordinal, s)| s.name.map(|name| (ordinal, name)))
        .collect();
    if named.is_empty() {
        if is_workbook {
            bail!(
                "no readable worksheets in this {} document: every sheet is empty \
                 (empty worksheets are omitted)",
                parsed.metadata.format
            );
        }
        bail!(
            "'sheet' only applies to spreadsheets (xlsx/xls); this {} document has no sheets",
            parsed.metadata.format
        );
    }
    let wanted_folded = wanted.to_lowercase();
    let (ordinal, name) = named
        .iter()
        .find(|(_, name)| name.to_lowercase() == wanted_folded)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "sheet '{wanted}' not found among non-empty sheets (empty worksheets are \
                 omitted); available: {}",
                named
                    .iter()
                    .map(|(_, n)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    let chunk = parsed
        .chunks
        .iter()
        .filter(|c| c.chunk_type == "sheet")
        .nth(*ordinal)
        .ok_or_else(|| anyhow::anyhow!("sheet '{name}' has no parsed content"))?;
    Ok((chunk.content.clone(), Some(name.clone())))
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
    parsed: &ExtractionResult,
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
        format: parsed.metadata.format.clone(),
        document_type: parsed.document_type.clone(),
        size_bytes: parsed.metadata.size_bytes,
        sections: sections_of(parsed),
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
        if matches!(window.format.as_str(), "xlsx" | "xls") {
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

    fn fake_parsed_with(
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

    fn fake_parsed(content: &str, chunks: Vec<(&str, &str)>) -> ExtractionResult {
        fake_parsed_with("xlsx", "excel", content, chunks)
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
        // The serde shape matches the schema: a bare path is a valid request.
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
    fn test_sections_follow_parser_layout_with_offsets() {
        let parsed = fake_parsed(
            "=== Sheet: Budget ===\na\tb\n\n=== Sheet: Notes ===\nc\n\n",
            vec![("sheet", "a\tb"), ("sheet", "c")],
        );
        assert_eq!(sheet_names(&parsed), vec!["Budget", "Notes"]);
        let sections = sections_of(&parsed);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name.as_deref(), Some("Budget"));
        assert_eq!(sections[0].offset, Some(22));
        assert_eq!(sections[0].chars, 3);
        assert_eq!(sections[1].name.as_deref(), Some("Notes"));
        assert_eq!(sections[1].offset, Some(48));
        assert_eq!(sections[1].index, 1);
        // The offsets point at each sheet's body inside the paged text.
        let body: String = parsed.content.chars().skip(22).take(3).collect();
        assert_eq!(body, "a\tb");
        let body: String = parsed.content.chars().skip(48).take(1).collect();
        assert_eq!(body, "c");
    }

    #[test]
    fn test_sections_ignore_header_shaped_cell_text() {
        // A Budget cell contains a line that looks like a sheet header. The
        // layout walk anchors each header to its chunk, so no phantom
        // "Evil" sheet appears and Notes stays selectable.
        let spoofed = "Note:\n=== Sheet: Evil ===\nend";
        let content = format!("=== Sheet: Budget ===\n{spoofed}\n\n=== Sheet: Notes ===\nc\n\n");
        let parsed = fake_parsed(&content, vec![("sheet", spoofed), ("sheet", "c")]);
        assert_eq!(sheet_names(&parsed), vec!["Budget", "Notes"]);
        let sections = sections_of(&parsed);
        assert_eq!(sections[1].name.as_deref(), Some("Notes"));
        let (text, section) = select_text(&parsed, Some("Notes")).unwrap();
        assert_eq!(text, "c");
        assert_eq!(section.as_deref(), Some("Notes"));
        let err = select_text(&parsed, Some("Evil")).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
        assert!(err.to_string().contains("Budget, Notes"), "{err}");
    }

    #[test]
    fn test_sections_without_offsets_when_layout_does_not_match() {
        let parsed = fake_parsed("unexpected layout\n", vec![("sheet", "a"), ("sheet", "b")]);
        let sections = sections_of(&parsed);
        assert_eq!(sections.len(), 2);
        assert!(sections
            .iter()
            .all(|s| s.name.is_none() && s.offset.is_none()));
        assert_eq!(sections[0].chars, 1);
        assert!(sheet_names(&parsed).is_empty());
    }

    #[test]
    fn test_csv_and_paragraph_sections_cover_the_whole_text() {
        let csv = fake_parsed_with("csv", "excel", "a,b\n1,2\n", vec![("csv", "a,b\n1,2\n")]);
        let sections = sections_of(&csv);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].offset, Some(0));
        assert_eq!(sections[0].chars, 8);

        // Word: paragraphs one per line in content, grouped into chunks
        // joined by a blank line.
        let content = "Dear tenant,\nYour lease renews.\n[Table]\nRegards\n";
        let docx = fake_parsed_with(
            "docx",
            "word",
            content,
            vec![
                ("paragraph", "Dear tenant,\n\nYour lease renews.\n\n[Table]"),
                ("paragraph", "Regards"),
            ],
        );
        let sections = sections_of(&docx);
        assert_eq!(sections[0].offset, Some(0));
        assert_eq!(sections[0].chars, 13 + 19 + 8);
        assert_eq!(sections[1].offset, Some(40));
        assert_eq!(sections[1].chars, 8);
        assert_eq!(
            sections.iter().map(|s| s.chars).sum::<usize>(),
            content.chars().count()
        );
        let tail: String = content.chars().skip(40).collect();
        assert_eq!(tail, "Regards\n");
    }

    #[test]
    fn test_paragraph_offsets_are_dropped_when_reconstruction_fails() {
        let docx = fake_parsed_with(
            "docx",
            "word",
            "Something the parser would not have produced",
            vec![("paragraph", "Dear tenant,\n\nRegards")],
        );
        let sections = sections_of(&docx);
        assert_eq!(sections[0].offset, None);
        assert_eq!(sections[0].chars, "Dear tenant,\n\nRegards".chars().count());
    }

    #[test]
    fn test_select_text_picks_sheet_case_insensitively_including_non_ascii() {
        let parsed = fake_parsed(
            "=== Sheet: Budget ===\na\tb\n\n=== Sheet: Übersicht ===\nc\n\n",
            vec![("sheet", "a\tb"), ("sheet", "c")],
        );
        let (text, section) = select_text(&parsed, Some("übersicht")).unwrap();
        assert_eq!(text, "c");
        assert_eq!(section.as_deref(), Some("Übersicht"));
        let (text, section) = select_text(&parsed, Some("BUDGET")).unwrap();
        assert_eq!(text, "a\tb");
        assert_eq!(section.as_deref(), Some("Budget"));
        let (all, none) = select_text(&parsed, None).unwrap();
        assert_eq!(all, parsed.content);
        assert_eq!(none, None);
    }

    #[test]
    fn test_select_text_errors_are_truthful_about_empty_and_non_spreadsheet_documents() {
        let empty_workbook = fake_parsed("", vec![]);
        let err = select_text(&empty_workbook, Some("Sheet1")).unwrap_err();
        assert!(err.to_string().contains("every sheet is empty"), "{err}");

        let csv = fake_parsed_with(
            "csv",
            "excel",
            "=== Sheet: X ===\n",
            vec![("csv", "=== Sheet: X ===\n")],
        );
        let err = select_text(&csv, Some("X")).unwrap_err();
        assert!(
            err.to_string().contains("only applies to spreadsheets"),
            "{err}"
        );

        let parsed = fake_parsed("=== Sheet: Budget ===\na\n\n", vec![("sheet", "a")]);
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
                    kind: "sheet".into(),
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

    #[cfg(feature = "office-support")]
    mod fixtures {
        use super::*;

        fn ctx_in(dir: &tempfile::TempDir) -> ReplContext {
            ReplContext {
                repo_root: dir.path().to_path_buf(),
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
            // A multi-line cell that looks like a sheet header must not
            // become a phantom sheet.
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
        async fn test_run_reads_xlsx_sheets_selects_one_and_ignores_spoofed_headers() {
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
            // The empty "Scratch" sheet is omitted by the parser; the spoofed
            // cell does not add an "Evil" sheet; indexes are workbook positions.
            assert_eq!(names, ["Budget", "Notes"]);
            assert_eq!(sections[1]["index"], 2);
            assert!(sections.iter().all(|s| s["offset"].is_u64()));
            let content = all.payload["content"].as_str().unwrap();
            assert!(content.contains("Rent") && content.contains("Paid on the first"));
            assert!(all.rendered.contains("empty worksheets are omitted"));

            let notes = DocumentReadTool
                .run(json!({"path": "ledger.xlsx", "sheet": "notes"}), &ctx)
                .await
                .unwrap();
            assert_eq!(notes.payload["section"], "Notes");
            let content = notes.payload["content"].as_str().unwrap();
            assert!(content.contains("Paid on the first"));
            assert!(!content.contains("Rent"));

            let err = DocumentReadTool
                .run(json!({"path": "ledger.xlsx", "sheet": "Scratch"}), &ctx)
                .await
                .unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("Budget, Notes") && msg.contains("empty worksheets are omitted"),
                "{msg}"
            );
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
        }
    }
}
