//! Excel/Word document I/O (feature-gated: `office-support`).
//!
//! Provides parsing, extraction, and generation for `.xlsx` and `.docx` files.
//! Used by the context pipeline to ingest Office documents as structured data.

pub mod excel;
pub mod extraction;
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

    /// Check if format is supported for reading
    pub fn is_readable(&self) -> bool {
        matches!(
            self,
            OfficeFormat::Xlsx | OfficeFormat::Xls | OfficeFormat::Csv | OfficeFormat::Docx
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
/// This is the main entry point for document parsing.
/// Uses feature flags to determine which parsers are available.
pub fn parse_document(path: &std::path::Path) -> Result<ExtractionResult, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or("No file extension")?;

    let format = OfficeFormat::from_extension(ext);

    if !format.is_readable() {
        return Err(format!("Unsupported format: {}", ext));
    }

    // Verify file exists and is readable
    let _metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;

    let result = match format {
        OfficeFormat::Xlsx | OfficeFormat::Xls | OfficeFormat::Csv => excel::parse_excel(path)?,
        OfficeFormat::Docx => word::parse_word(path)?,
        OfficeFormat::Unknown | OfficeFormat::Doc => {
            return Err(format!("Format {} not supported", ext));
        }
    };

    Ok(result)
}

/// List supported Office formats
pub fn supported_formats() -> Vec<(&'static str, &'static str, bool, bool)> {
    vec![
        ("xlsx", "Excel (modern)", true, true),
        ("xls", "Excel (legacy)", true, false),
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
}
