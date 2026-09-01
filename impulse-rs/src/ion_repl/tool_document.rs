//! `document_read` as a `ReplTool`: bounded, pageable document analysis
//! for Ion.
//!
//! The `office` module can already parse spreadsheets and Word files, and
//! `src/tooling/document` exposes that to the CLI and daemon, but neither
//! was reachable from Ion's tool loop, and both return whole documents in
//! one unbounded payload. A model working inside a loop contract
//! (ADR-0017) needs the opposite shape: a short outline first, then
//! windows of content it can page through with an explicit `offset`, so
//! one large invoice or workbook never floods the context budget.
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
pub const DEFAULT_MAX_CHARS: usize = 8_000;
/// Hard ceiling on characters returned per call, whatever the model asks.
pub const MAX_CHARS_CAP: usize = 32_000;

const SHEET_HEADER_PREFIX: &str = "=== Sheet: ";
const SHEET_HEADER_SUFFIX: &str = " ===";

pub struct DocumentReadTool;

/// Validated arguments for one `document_read` call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentReadRequest {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub outline: bool,
    pub offset: usize,
    pub max_chars: usize,
}

/// One addressable part of a document: a sheet, a CSV body, or a run of
/// paragraphs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSection {
    pub index: usize,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
         \"offset\": 0, \"max_chars\": 8000} -- read a spreadsheet or Word document \
         (xlsx/xls/csv/docx) as text, paged by character offset"
    }

    fn json_schema(&self) -> Value {
        json!({
            "name": "document_read",
            "description": "Read a spreadsheet (xlsx, xls, csv) or Word document (docx) as \
                plain text. Read-only. Start with outline=true to see the sections and total \
                size, then read windows of at most max_chars characters and continue from \
                next_offset until truncated is false. Use sheet to read one worksheet.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Document path; relative paths resolve against the REPL's launch directory"
                    },
                    "sheet": {
                        "type": "string",
                        "description": "Worksheet name to read (spreadsheets only, case-insensitive)"
                    },
                    "outline": {
                        "type": "boolean",
                        "description": "Return sections and sizes only, no content",
                        "default": false
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Character offset to start from (use next_offset from the previous call)",
                        "default": 0
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": format!("Characters to return, default {DEFAULT_MAX_CHARS}, capped at {MAX_CHARS_CAP}"),
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
        let parsed = office::parse_document(&path)
            .map_err(|e| anyhow::anyhow!("document_read failed for {}: {e}", path.display()))?;
        let window = build_window(&request, &path, &parsed)?;
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
/// existing regular file in a readable format. Errors name the path and,
/// for an unsupported extension, list the supported ones.
pub fn resolve_document_path(raw: &str, repo_root: &Path) -> Result<PathBuf> {
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
        let supported: Vec<&str> = office::supported_formats()
            .into_iter()
            .filter(|(_, _, readable, _)| *readable)
            .map(|(ext, _, _, _)| ext)
            .collect();
        bail!(
            "document_read cannot read '{}': unsupported extension '{ext}' (supported: {})",
            path.display(),
            supported.join(", ")
        );
    }
    let metadata = std::fs::metadata(&path)
        .with_context(|| format!("document_read cannot find '{}'", path.display()))?;
    if !metadata.is_file() {
        bail!(
            "document_read expects a file, got a directory: '{}'",
            path.display()
        );
    }
    Ok(path)
}

/// Sheet names in chunk order, recovered from the parser's own section
/// headers so the mapping is exact for every non-empty sheet.
pub fn sheet_names(parsed: &ExtractionResult) -> Vec<String> {
    parsed
        .content
        .lines()
        .filter_map(|line| {
            line.strip_prefix(SHEET_HEADER_PREFIX)
                .and_then(|rest| rest.strip_suffix(SHEET_HEADER_SUFFIX))
        })
        .map(str::to_string)
        .collect()
}

/// The addressable sections of a parsed document.
pub fn sections_of(parsed: &ExtractionResult) -> Vec<DocumentSection> {
    let names = sheet_names(parsed);
    parsed
        .chunks
        .iter()
        .enumerate()
        .map(|(position, chunk)| DocumentSection {
            index: chunk.index,
            kind: chunk.chunk_type.clone(),
            name: if chunk.chunk_type == "sheet" {
                names.get(position).cloned()
            } else {
                None
            },
            chars: chunk.content.chars().count(),
        })
        .collect()
}

/// The text a call reads from: the whole document, or one sheet when
/// `sheet` is given. Sheet selection is only meaningful for spreadsheets.
pub fn select_text(
    parsed: &ExtractionResult,
    sheet: Option<&str>,
) -> Result<(String, Option<String>)> {
    let Some(wanted) = sheet else {
        return Ok((parsed.content.clone(), None));
    };
    let names = sheet_names(parsed);
    if names.is_empty() {
        bail!(
            "'sheet' only applies to spreadsheets with named sheets; this {} document has none",
            parsed.metadata.format
        );
    }
    let position = names
        .iter()
        .position(|name| name.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "sheet '{wanted}' not found; available sheets: {}",
                names.join(", ")
            )
        })?;
    let chunk = parsed
        .chunks
        .iter()
        .filter(|c| c.chunk_type == "sheet")
        .nth(position)
        .ok_or_else(|| anyhow::anyhow!("sheet '{wanted}' has no parsed content"))?;
    Ok((chunk.content.clone(), Some(names[position].clone())))
}

/// A character window over `text`. Offsets past the end yield an empty,
/// non-truncated window rather than an error so a model that overshoots
/// learns it is done.
pub fn window(text: &str, offset: usize, max_chars: usize) -> (String, usize, bool, Option<usize>) {
    let total = text.chars().count();
    let start = offset.min(total);
    let content: String = text.chars().skip(start).take(max_chars).collect();
    let returned = content.chars().count();
    let truncated = start + returned < total;
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
/// section table, then the content window with an explicit continuation
/// hint when more remains.
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
    for section in &window.sections {
        match &section.name {
            Some(name) => out.push_str(&format!(
                "  [{}] {} \"{}\" {} chars\n",
                section.index, section.kind, name, section.chars
            )),
            None => out.push_str(&format!(
                "  [{}] {} {} chars\n",
                section.index, section.kind, section.chars
            )),
        }
    }
    match &window.content {
        None => out.push_str("(outline only; call again without outline to read content)\n"),
        Some(content) => {
            let end = window.offset + window.returned_chars;
            if window.truncated {
                out.push_str(&format!(
                    "--- content chars {}..{} of {}; truncated, continue with offset={} ---\n",
                    window.offset,
                    end,
                    window.total_chars,
                    window.next_offset.unwrap_or(end)
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
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{ContentChunk, ExtractionMetadata};

    fn fake_parsed(content: &str, chunks: Vec<(&str, &str)>) -> ExtractionResult {
        ExtractionResult {
            document_type: "excel".into(),
            content: content.to_string(),
            metadata: ExtractionMetadata {
                source_path: "fake.xlsx".into(),
                format: "xlsx".into(),
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
    fn test_window_truncates_and_reports_next_offset() {
        let (content, returned, truncated, next) = window("abcdefgh", 2, 3);
        assert_eq!(content, "cde");
        assert_eq!(returned, 3);
        assert!(truncated);
        assert_eq!(next, Some(5));
        let (rest, _, truncated, next) = window("abcdefgh", 5, 3);
        assert_eq!(rest, "fgh");
        assert!(!truncated);
        assert_eq!(next, None);
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
    fn test_sheet_names_and_sections_follow_parser_headers() {
        let parsed = fake_parsed(
            "=== Sheet: Budget ===\na\tb\n\n=== Sheet: Notes ===\nc\n\n",
            vec![("sheet", "a\tb"), ("sheet", "c")],
        );
        assert_eq!(sheet_names(&parsed), vec!["Budget", "Notes"]);
        let sections = sections_of(&parsed);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name.as_deref(), Some("Budget"));
        assert_eq!(sections[0].chars, 3);
        assert_eq!(sections[1].name.as_deref(), Some("Notes"));
        assert_eq!(sections[1].index, 1);
    }

    #[test]
    fn test_select_text_picks_sheet_case_insensitively() {
        let parsed = fake_parsed(
            "=== Sheet: Budget ===\na\tb\n\n=== Sheet: Notes ===\nc\n\n",
            vec![("sheet", "a\tb"), ("sheet", "c")],
        );
        let (text, section) = select_text(&parsed, Some("notes")).unwrap();
        assert_eq!(text, "c");
        assert_eq!(section.as_deref(), Some("Notes"));
        let (all, none) = select_text(&parsed, None).unwrap();
        assert_eq!(all, parsed.content);
        assert_eq!(none, None);
    }

    #[test]
    fn test_select_text_errors_list_available_sheets() {
        let parsed = fake_parsed("=== Sheet: Budget ===\na\n\n", vec![("sheet", "a")]);
        let err = select_text(&parsed, Some("Missing")).unwrap_err();
        assert!(err.to_string().contains("Budget"), "{err}");
        let csv = fake_parsed("a,b\n", vec![("csv", "a,b\n")]);
        let err = select_text(&csv, Some("any")).unwrap_err();
        assert!(
            err.to_string().contains("only applies to spreadsheets"),
            "{err}"
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
        assert!(err.to_string().contains("missing.csv"), "{err}");

        std::fs::write(dir.path().join("notes.pdf"), "%PDF").unwrap();
        let err = resolve_document_path("notes.pdf", dir.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unsupported extension 'pdf'"), "{msg}");
        assert!(
            msg.contains("xlsx") && msg.contains("csv") && msg.contains("docx"),
            "{msg}"
        );

        std::fs::create_dir(dir.path().join("folder.csv")).unwrap();
        let err = resolve_document_path("folder.csv", dir.path()).unwrap_err();
        assert!(err.to_string().contains("directory"), "{err}");
    }

    #[test]
    fn round_trip_document_window_and_request() {
        let window = DocumentWindow {
            path: "a.xlsx".into(),
            format: "xlsx".into(),
            document_type: "excel".into(),
            size_bytes: 10,
            sections: vec![DocumentSection {
                index: 0,
                kind: "sheet".into(),
                name: Some("Budget".into()),
                chars: 3,
            }],
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
    fn test_render_shows_sections_and_continuation_hint() {
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
                    chars: 30,
                },
                DocumentSection {
                    index: 0,
                    kind: "csv".into(),
                    name: None,
                    chars: 5,
                },
            ],
            section: None,
            total_chars: 30,
            offset: 0,
            returned_chars: 10,
            truncated: true,
            next_offset: Some(10),
            content: Some("0123456789".into()),
        };
        let text = render(&window);
        assert!(text.contains("[0] sheet \"Budget\" 30 chars"), "{text}");
        assert!(text.contains("[0] csv 5 chars"), "{text}");
        assert!(text.contains("continue with offset=10"), "{text}");
        assert!(text.ends_with("0123456789\n"), "{text}");

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
            assert!(outcome.payload["content"]
                .as_str()
                .unwrap()
                .contains("Grocer"));
            assert!(outcome.rendered.contains("expenses.csv"));
            assert!(outcome.rendered.contains("Grocer"));
            assert!(outcome.rendered.contains("complete"));
        }

        #[tokio::test]
        async fn test_run_pages_through_csv_with_offsets() {
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
                assembled.push_str(outcome.payload["content"].as_str().unwrap());
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
        async fn test_run_reads_xlsx_sheets_and_selects_one() {
            let dir = tempfile::TempDir::new().unwrap();
            let path = write_workbook(&dir);
            let ctx = ctx_in(&dir);

            let all = DocumentReadTool
                .run(json!({"path": path.to_str().unwrap()}), &ctx)
                .await
                .unwrap();
            let names: Vec<&str> = all.payload["sections"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["name"].as_str().unwrap())
                .collect();
            assert_eq!(names, ["Budget", "Notes"]);
            let content = all.payload["content"].as_str().unwrap();
            assert!(content.contains("Rent") && content.contains("Paid on the first"));

            let notes = DocumentReadTool
                .run(json!({"path": "ledger.xlsx", "sheet": "notes"}), &ctx)
                .await
                .unwrap();
            assert_eq!(notes.payload["section"], "Notes");
            let content = notes.payload["content"].as_str().unwrap();
            assert!(content.contains("Paid on the first"));
            assert!(!content.contains("Rent"));

            let err = DocumentReadTool
                .run(json!({"path": "ledger.xlsx", "sheet": "Taxes"}), &ctx)
                .await
                .unwrap_err();
            assert!(err.to_string().contains("Budget, Notes"), "{err}");
        }

        #[tokio::test]
        async fn test_run_reads_docx_paragraphs() {
            let dir = tempfile::TempDir::new().unwrap();
            write_docx(&dir);

            let outcome = DocumentReadTool
                .run(json!({"path": "letter.docx"}), &ctx_in(&dir))
                .await
                .unwrap();

            assert_eq!(outcome.payload["format"], "docx");
            assert_eq!(outcome.payload["document_type"], "word");
            assert_eq!(outcome.payload["sections"][0]["kind"], "paragraph");
            let content = outcome.payload["content"].as_str().unwrap();
            assert!(content.contains("Dear tenant"), "{content}");
            assert!(content.contains("renews in March"), "{content}");
        }

        #[tokio::test]
        async fn test_run_surfaces_argument_and_path_errors() {
            let dir = tempfile::TempDir::new().unwrap();
            let ctx = ctx_in(&dir);
            let err = DocumentReadTool.run(json!({}), &ctx).await.unwrap_err();
            assert!(err.to_string().contains("'path'"), "{err}");
            let err = DocumentReadTool
                .run(json!({"path": "nope.docx"}), &ctx)
                .await
                .unwrap_err();
            assert!(err.to_string().contains("nope.docx"), "{err}");
        }
    }
}
