// Word module - parse and extract data from Word documents
// Uses docx-rs for reading .docx files

use crate::office::ExtractionResult;

#[cfg(feature = "office-support")]
use docx::*;
#[cfg(feature = "office-support")]
use std::path::Path;

/// Parse a Word document and extract content
#[cfg(feature = "office-support")]
pub fn parse_word(path: &std::path::Path) -> Result<ExtractionResult, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let doc = docx::read_docx(&bytes).map_err(|e| format!("Failed to parse DOCX: {}", e))?;

    extract_word_content(path, doc)
}

#[cfg(not(feature = "office-support"))]
pub fn parse_word(_path: &std::path::Path) -> Result<ExtractionResult, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
}

#[cfg(feature = "office-support")]
/// Extract content from parsed Word document
fn extract_word_content(path: &Path, doc: Docx) -> Result<ExtractionResult, String> {
    let mut paragraphs = Vec::new();
    let mut chunks = Vec::new();
    let mut content = String::new();

    for child in &doc.document.children {
        extract_child_elements(child, &mut paragraphs, &mut content);
    }

    let chunk_size = 10;
    for (idx, chunk) in paragraphs.chunks(chunk_size).enumerate() {
        let chunk_content = chunk.join("\n\n");
        chunks.push(ContentChunk {
            content: chunk_content.clone(),
            chunk_type: "paragraph".to_string(),
            index: idx,
        });
    }

    let _word_count = content.split_whitespace().count();
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;

    Ok(ExtractionResult {
        document_type: "word".to_string(),
        content: content.clone(),
        metadata: ExtractionMetadata {
            source_path: path.to_string_lossy().to_string(),
            format: "docx".to_string(),
            size_bytes: metadata.len(),
            extracted_at: chrono::Utc::now().to_rfc3339(),
        },
        chunks,
    })
}

#[cfg(feature = "office-support")]
fn extract_child_elements(
    child: &DocumentChild,
    paragraphs: &mut Vec<String>,
    content: &mut String,
) {
    match child {
        DocumentChild::Paragraph(p) => {
            let mut para_content = String::new();
            for run in &p.children {
                if let ParagraphChild::Run(run) = run {
                    for elem in &run.children {
                        if let RunChild::Text(t) = elem {
                            para_content.push_str(&t.text);
                        }
                    }
                }
            }

            let trimmed = para_content.trim();
            if !trimmed.is_empty() {
                paragraphs.push(trimmed.to_string());
                content.push_str(trimmed);
                content.push('\n');
            }
        }
        DocumentChild::Table(_t) => {
            // Table extraction - simplified for now
            let table_para = "[Table]".to_string();
            paragraphs.push(table_para.clone());
            content.push_str(&table_para);
            content.push('\n');
        }
        _ => {}
    }
}

/// Get document statistics without full parsing
#[cfg(feature = "office-support")]
pub fn get_word_stats(path: &std::path::Path) -> Result<WordStats, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let doc = docx::read_docx(&bytes).map_err(|e| format!("Failed to parse DOCX: {}", e))?;

    let mut paragraph_count = 0;
    let mut table_count = 0;

    for child in &doc.document.children {
        match child {
            DocumentChild::Paragraph(_) => paragraph_count += 1,
            DocumentChild::Table(_) => table_count += 1,
            _ => {}
        }
    }

    let content = extract_text_from_doc(&doc);
    let word_count = content.split_whitespace().count();

    Ok(WordStats {
        paragraph_count,
        word_count,
        table_count,
    })
}

#[cfg(not(feature = "office-support"))]
pub fn get_word_stats(_path: &std::path::Path) -> Result<WordStats, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
}

#[cfg(feature = "office-support")]
fn extract_text_from_doc(doc: &Docx) -> String {
    let mut content = String::new();

    for child in &doc.document.children {
        if let DocumentChild::Paragraph(p) = child {
            for run in &p.children {
                if let ParagraphChild::Run(run) = run {
                    for elem in &run.children {
                        if let RunChild::Text(t) = elem {
                            content.push_str(&t.text);
                            content.push(' ');
                        }
                    }
                }
            }
            content.push('\n');
        }
    }

    content
}

/// Statistics about a Word document
#[derive(Debug, Clone)]
pub struct WordStats {
    pub paragraph_count: usize,
    pub word_count: usize,
    pub table_count: usize,
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_office_format() {
        assert_eq!(
            super::super::OfficeFormat::from_extension("docx"),
            super::super::OfficeFormat::Docx
        );
    }
}
