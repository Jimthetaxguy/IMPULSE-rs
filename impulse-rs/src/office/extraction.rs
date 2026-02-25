// Extraction module - document extraction pipeline
// Combines Office document parsing with structured content chunking

use crate::office::ContentChunk;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configuration for document extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionConfig {
    /// Maximum chunk size for content splitting
    pub max_chunk_size: usize,
    /// Overlap between chunks
    pub chunk_overlap: usize,
    /// Include metadata in extraction
    pub include_metadata: bool,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            max_chunk_size: 4000,
            chunk_overlap: 200,
            include_metadata: true,
        }
    }
}

/// A structured extraction target for Monty to analyze
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionTarget {
    /// The document content to analyze
    pub content: String,
    /// Type of document
    pub document_type: String,
    /// What to extract (e.g., "financial data", "contact info", "dates")
    pub extraction_goal: String,
    /// Source file path
    pub source: String,
}

impl ExtractionTarget {
    pub fn new(content: &str, doc_type: &str, goal: &str, source: &str) -> Self {
        Self {
            content: content.to_string(),
            document_type: doc_type.to_string(),
            extraction_goal: goal.to_string(),
            source: source.to_string(),
        }
    }
}

/// Result of intelligent extraction by Monty
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelligentExtraction {
    /// Extracted entities or data
    pub findings: Vec<ExtractedFinding>,
    /// Summary of the document
    pub summary: String,
    /// Suggested next steps
    pub recommendations: Vec<String>,
    /// Confidence score
    pub confidence: f64,
}

/// A single extracted piece of data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFinding {
    /// Type of finding (e.g., "date", "amount", "name")
    pub finding_type: String,
    /// The extracted value
    pub value: String,
    /// Location in document (optional)
    pub location: Option<String>,
    /// Confidence in this extraction
    pub confidence: f64,
}

/// Parse document and create extraction target
pub fn create_extraction_target(path: &Path, goal: &str) -> Result<ExtractionTarget, String> {
    let result = crate::office::parse_document(path)?;

    Ok(ExtractionTarget::new(
        &result.content,
        &result.document_type,
        goal,
        &path.to_string_lossy(),
    ))
}

/// Find the nearest valid UTF-8 char boundary at or after `pos`.
fn ceil_char_boundary(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    // Walk forward until we hit a char boundary
    let mut i = pos;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Find the nearest valid UTF-8 char boundary at or before `pos`.
fn floor_char_boundary(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    let mut i = pos;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Chunk content for processing.
/// Uses char-boundary-safe slicing to avoid panics on multi-byte UTF-8 content.
pub fn chunk_content(content: &str, max_size: usize, overlap: usize) -> Vec<ContentChunk> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut index = 0;

    while start < content.len() {
        let raw_end = (start + max_size).min(content.len());
        let end = ceil_char_boundary(content, raw_end);
        let end = end.min(content.len());
        let chunk_content = &content[start..end];

        chunks.push(ContentChunk {
            content: chunk_content.to_string(),
            chunk_type: "text".to_string(),
            index,
        });

        if end == content.len() {
            break;
        }

        // Move start forward with overlap, ensuring we land on a char boundary
        let raw_next = if end > overlap { end - overlap } else { end };
        start = floor_char_boundary(content, raw_next);
        // Prevent infinite loop if overlap >= max_size
        if start <= chunks.last().map(|c| c.index).unwrap_or(0) && start == 0 && index > 0 {
            break;
        }
        index += 1;
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extraction_config_default() {
        let config = ExtractionConfig::default();
        assert_eq!(config.max_chunk_size, 4000);
        assert!(config.include_metadata);
    }

    #[test]
    fn test_extraction_target_new() {
        let target = ExtractionTarget::new("test content", "excel", "extract dates", "test.xlsx");
        assert_eq!(target.document_type, "excel");
        assert_eq!(target.extraction_goal, "extract dates");
    }

    #[test]
    fn test_chunk_content() {
        let content = "0123456789".repeat(100);
        let chunks = chunk_content(&content, 50, 10);
        assert!(chunks.len() > 1);
        assert_eq!(chunks[0].chunk_type, "text");
    }

    #[test]
    fn test_chunk_content_multibyte_utf8() {
        // Content with multi-byte characters (em-dashes, accented chars, CJK)
        let content = "Hello — world! Café résumé 日本語テスト end";
        // Use a small chunk size that would land mid-character without boundary checks
        let chunks = chunk_content(content, 10, 2);
        assert!(!chunks.is_empty());
        // Verify all chunks are valid UTF-8 (would panic if boundary was wrong)
        for chunk in &chunks {
            assert!(chunk.content.is_char_boundary(0));
        }
    }
}
