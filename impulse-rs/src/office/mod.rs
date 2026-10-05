//! Excel/Word document I/O (feature-gated: `office-support`).
//!
//! Provides parsing, extraction, and generation for `.xlsx` and `.docx` files.
//! Used by the context pipeline to ingest Office documents as structured data.
//! Every read goes through the bounded readers in `office::bounded`, which
//! Ion's `document_read` shares: a file-size cap, a container inflation cap,
//! and streaming workbook and Word readers. Legacy `.xls` is refused.

#[cfg(feature = "office-support")]
pub mod bounded;
pub mod excel;
pub mod extraction;
#[cfg(all(test, feature = "office-support"))]
pub(crate) mod test_fixtures;
pub mod word;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Represents a parsed Office document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OfficeDocument {
    Excel(ExcelDocument),
    Word(WordDocument),
    Unknown,
}

/// Parsed Excel document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcelDocument {
    pub path: PathBuf,
    pub sheets: Vec<SheetInfo>,
    pub row_count: usize,
    pub column_count: usize,
}

/// Information about an Excel sheet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetInfo {
    pub name: String,
    pub row_count: usize,
    pub column_count: usize,
}

/// Parsed Word document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordDocument {
    pub path: PathBuf,
    pub paragraphs: Vec<String>,
    pub word_count: usize,
}

/// Supported Office formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OfficeFormat {
    Xlsx,
    Xls,
    Csv,
    Docx,
    Doc,
    Unknown,
}

impl OfficeFormat {
    /// Detect format from file extension
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "xlsx" => OfficeFormat::Xlsx,
            "xls" => OfficeFormat::Xls,
            "csv" => OfficeFormat::Csv,
            "docx" => OfficeFormat::Docx,
            "doc" => OfficeFormat::Doc,
            _ => OfficeFormat::Unknown,
        }
    }

    /// Check if format is supported for reading. Legacy `.xls` is not: its
    /// binary format has no streaming reader, so it cannot be read under the
    /// office bounds.
    pub fn is_readable(&self) -> bool {
        matches!(
            self,
            OfficeFormat::Xlsx | OfficeFormat::Csv | OfficeFormat::Docx
        )
    }

    /// Check if format is supported for writing
    pub fn is_writable(&self) -> bool {
        matches!(
            self,
            OfficeFormat::Xlsx | OfficeFormat::Csv | OfficeFormat::Docx
        )
    }

    /// Get string representation of format
    pub fn as_str(&self) -> &'static str {
        match self {
            OfficeFormat::Xlsx => "xlsx",
            OfficeFormat::Xls => "xls",
            OfficeFormat::Csv => "csv",
            OfficeFormat::Docx => "docx",
            OfficeFormat::Doc => "doc",
            OfficeFormat::Unknown => "unknown",
        }
    }
}

/// Result of document extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub document_type: String,
    pub content: String,
    pub metadata: ExtractionMetadata,
    pub chunks: Vec<ContentChunk>,
}

/// Metadata about extracted document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionMetadata {
    pub source_path: String,
    pub format: String,
    pub size_bytes: u64,
    pub extracted_at: String,
}

/// A chunk of extracted content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentChunk {
    pub content: String,
    pub chunk_type: String,
    pub index: usize,
}

/// Parse an Office document and return extracted content
///
/// This is the main entry point for document parsing. Every format is read
/// under the bounds in `office::bounded`; legacy `.xls` is refused with the
/// reason.
pub fn parse_document(path: &std::path::Path) -> Result<ExtractionResult, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or("No file extension")?;

    match OfficeFormat::from_extension(ext) {
        OfficeFormat::Xlsx | OfficeFormat::Xls | OfficeFormat::Csv => excel::parse_excel(path),
        OfficeFormat::Docx => word::parse_word(path),
        OfficeFormat::Doc | OfficeFormat::Unknown => Err(format!("Unsupported format: {}", ext)),
    }
}

/// Reads `path` under the bounds in [`bounded`] and adapts the result for
/// the office callers.
#[cfg(feature = "office-support")]
fn read_bounded(path: &std::path::Path) -> Result<ExtractionResult, String> {
    let parsed = bounded::read_document(
        path,
        &bounded::path_label(path),
        bounded::ExtractBudget::DEFAULT,
    )
    .map_err(|e| format!("{e:#}"))?;
    Ok(extraction_result(path, parsed))
}

/// Adapts a bounded read to an [`ExtractionResult`]. Each chunk is the exact
/// span of `content` one section covers: a sheet's body, the whole of a CSV
/// file, or a run of Word lines.
#[cfg(feature = "office-support")]
fn extraction_result(path: &std::path::Path, parsed: bounded::ParsedDocument) -> ExtractionResult {
    let mut chunks = Vec::with_capacity(parsed.sections.len());
    // Sections arrive in text order and do not overlap, so one pass over the
    // text finds every span.
    let mut rest = parsed.text.as_str();
    let mut consumed = 0usize;
    for section in &parsed.sections {
        let Some(skip) = section
            .offset
            .and_then(|offset| offset.checked_sub(consumed))
        else {
            continue;
        };
        let (_, from_offset) = split_after_chars(rest, skip);
        let (span, after) = split_after_chars(from_offset, section.chars);
        chunks.push(ContentChunk {
            content: span.to_string(),
            chunk_type: section.kind.clone(),
            index: section.index,
        });
        rest = after;
        consumed += skip + section.chars;
    }
    ExtractionResult {
        document_type: parsed.document_type,
        content: parsed.text,
        metadata: ExtractionMetadata {
            source_path: path.to_string_lossy().to_string(),
            format: parsed.format,
            size_bytes: parsed.size_bytes,
            extracted_at: chrono::Utc::now().to_rfc3339(),
        },
        chunks,
    }
}

/// Splits `s` after its first `chars` characters, or at its end.
#[cfg(feature = "office-support")]
fn split_after_chars(s: &str, chars: usize) -> (&str, &str) {
    let at = s
        .char_indices()
        .nth(chars)
        .map_or(s.len(), |(index, _)| index);
    s.split_at(at)
}

/// List supported Office formats
pub fn supported_formats() -> Vec<(&'static str, &'static str, bool, bool)> {
    vec![
        ("xlsx", "Excel (modern)", true, true),
        ("xls", "Excel (legacy)", false, false),
        ("csv", "CSV (Comma-separated)", true, true),
        ("docx", "Word (modern)", true, false),
        ("doc", "Word (legacy)", false, false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_office_format_detection() {
        assert_eq!(OfficeFormat::from_extension("xlsx"), OfficeFormat::Xlsx);
        assert_eq!(OfficeFormat::from_extension("XLSX"), OfficeFormat::Xlsx);
        assert_eq!(OfficeFormat::from_extension("docx"), OfficeFormat::Docx);
        assert_eq!(OfficeFormat::from_extension("csv"), OfficeFormat::Csv);
        assert_eq!(
            OfficeFormat::from_extension("unknown"),
            OfficeFormat::Unknown
        );
    }

    #[test]
    fn test_format_readable() {
        assert!(OfficeFormat::Xlsx.is_readable());
        assert!(OfficeFormat::Csv.is_readable());
        assert!(OfficeFormat::Docx.is_readable());
        assert!(!OfficeFormat::Doc.is_readable());
        assert!(!OfficeFormat::Unknown.is_readable());
    }

    #[test]
    fn test_format_writable() {
        assert!(OfficeFormat::Xlsx.is_writable());
        assert!(OfficeFormat::Csv.is_writable());
        assert!(!OfficeFormat::Xls.is_writable());
    }

    #[test]
    fn test_supported_formats() {
        let formats = supported_formats();
        assert!(!formats.is_empty());
        assert!(formats.iter().any(|(ext, _, _, _)| ext == &"xlsx"));
    }

    #[test]
    fn test_supported_formats_marks_legacy_xls_unreadable() {
        let formats = supported_formats();
        let xls = formats.iter().find(|(ext, ..)| *ext == "xls").unwrap();
        assert!(!xls.2, "{xls:?}");
        assert!(!OfficeFormat::Xls.is_readable());
    }

    /// The bounds `parse_document` applies to every format.
    #[cfg(feature = "office-support")]
    mod bounded_reads {
        use super::super::test_fixtures::*;
        use super::*;

        /// Legacy `.xls` was opened with calamine's `.xls` reader, which
        /// builds every sheet's dense grid as it opens the file.
        #[test]
        fn test_parse_document_refuses_legacy_xls() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("legacy.xls");
            std::fs::write(&path, b"not really a workbook").unwrap();
            let err = parse_document(&path).unwrap_err();
            assert!(err.contains("legacy .xls"), "{err}");
        }

        /// A file over the 10 MiB cap is refused before a parser reads it; a
        /// CSV used to be read whole, whatever its size.
        #[test]
        fn test_parse_document_refuses_a_file_over_the_size_cap() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("big.csv");
            let file = std::fs::File::create(&path).unwrap();
            file.set_len(10 * 1024 * 1024 + 1).unwrap();
            let err = parse_document(&path).unwrap_err();
            assert!(err.contains("over the 10485760-byte limit"), "{err}");
        }

        /// A container whose entries inflate past 64 MiB is refused, though
        /// the one part a Word reader needs is tiny: nothing used to inflate
        /// the other entries before parsing.
        #[test]
        fn test_parse_document_refuses_a_decompression_bomb() {
            let dir = tempfile::tempdir().unwrap();
            let path = write_docx_bomb(dir.path(), 64 * 1024 * 1024);
            assert!(std::fs::metadata(&path).unwrap().len() < 1024 * 1024);
            let err = parse_document(&path).unwrap_err();
            assert!(
                err.contains("inflates to more than 67108864 bytes"),
                "{err}"
            );
        }

        /// With overflow checks on, as in tests, calamine panics on an
        /// inverted `<dimension>` and on a cell reference whose row
        /// overflows `u32`. The office CLI and context provider call the
        /// reader synchronously, so the panic must fail the one document
        /// rather than unwind the caller.
        #[test]
        fn test_parse_document_contains_a_parser_panic() {
            let dir = tempfile::tempdir().unwrap();
            let inverted = write_workbook_with_edited_sheet(dir.path(), "inverted", |xml| {
                let start = xml.find("<dimension ref=\"").unwrap() + "<dimension ref=\"".len();
                let end = start + xml[start..].find('"').unwrap();
                format!("{}B2:A1{}", &xml[..start], &xml[end..])
            });
            let long_row = write_workbook_with_edited_sheet(dir.path(), "long_row", |xml| {
                assert!(xml.contains("<c r=\"A1\""), "{xml}");
                xml.replacen("<c r=\"A1\"", "<c r=\"A99999999999\"", 1)
            });
            for path in [&inverted, &long_row] {
                // Getting a result at all means no panic escaped.
                let result = parse_document(path);
                if cfg!(debug_assertions) {
                    let err = result.unwrap_err();
                    assert!(err.contains("the parser panicked"), "{err}");
                }
            }
            let result = excel::get_sheet_info(&inverted);
            if cfg!(debug_assertions) {
                let err = result.unwrap_err();
                assert!(err.contains("the parser panicked"), "{err}");
            }
        }

        /// Each chunk is the span of `content` one section covers, so a Word
        /// document's chunks tile its text, ten lines to a chunk.
        #[test]
        fn test_parse_document_word_chunks_tile_the_text() {
            let dir = tempfile::tempdir().unwrap();
            let path = write_docx_lines(dir.path(), 12);
            let result = parse_document(&path).unwrap();
            let lines: Vec<String> = (0..12).map(|i| format!("line {i}\n")).collect();
            assert_eq!(result.content, lines.concat());
            assert_eq!(result.chunks.len(), 2);
            assert_eq!(result.chunks[0].content, lines[..10].concat());
            assert_eq!(result.chunks[1].content, lines[10..].concat());
            assert!(result.chunks.iter().all(|c| c.chunk_type == "paragraph"));
            assert_eq!((result.chunks[0].index, result.chunks[1].index), (0, 1));
        }

        /// A workbook's chunks are its non-empty sheets' bodies, indexed by
        /// position in the workbook; a CSV's one chunk is the whole file.
        #[test]
        fn test_parse_document_sheet_and_csv_chunks() {
            let dir = tempfile::tempdir().unwrap();
            let workbook = write_far_apart_workbook(dir.path());
            let result = parse_document(&workbook).unwrap();
            assert_eq!(result.chunks.len(), 1);
            assert_eq!(result.chunks[0].chunk_type, "sheet");
            assert_eq!(result.chunks[0].index, 0);
            assert_eq!(
                result.content,
                format!("=== Sheet: Data ===\n{}\n\n", result.chunks[0].content)
            );

            let csv = dir.path().join("rows.csv");
            std::fs::write(&csv, "a,b\n1,2\n").unwrap();
            let result = parse_document(&csv).unwrap();
            assert_eq!(result.content, "a,b\n1,2\n");
            assert_eq!(result.metadata.format, "csv");
            let chunk = &result.chunks[..];
            assert_eq!(chunk.len(), 1);
            assert_eq!(
                (
                    chunk[0].content.as_str(),
                    chunk[0].chunk_type.as_str(),
                    chunk[0].index
                ),
                ("a,b\n1,2\n", "csv", 0)
            );
        }
    }
}
