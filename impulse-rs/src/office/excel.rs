// Excel module - parse and extract data from Excel files
// Uses calamine for reading Excel files

#[cfg(not(feature = "office-support"))]
use crate::office::ExtractionResult;
use crate::office::SheetInfo;
#[cfg(feature = "office-support")]
use crate::office::{ContentChunk, ExtractionMetadata, ExtractionResult};

#[cfg(feature = "office-support")]
use calamine::{open_workbook, Reader, Xls, Xlsx};
#[cfg(feature = "office-support")]
use std::path::Path;

/// Parse an Excel file and extract content
#[cfg(feature = "office-support")]
pub fn parse_excel(path: &std::path::Path) -> Result<ExtractionResult, String> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "xlsx" => parse_xlsx(path),
        "xls" => parse_xls(path),
        "csv" => parse_csv(path),
        _ => Err(format!("Unsupported Excel format: {}", extension)),
    }
}

#[cfg(not(feature = "office-support"))]
pub fn parse_excel(_path: &std::path::Path) -> Result<ExtractionResult, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
}

#[cfg(feature = "office-support")]
/// Parse XLSX file using calamine
fn parse_xlsx(path: &Path) -> Result<ExtractionResult, String> {
    let mut workbook: Xlsx<_> =
        open_workbook(path).map_err(|e| format!("Failed to open workbook: {}", e))?;

    extract_xlsx_content(path, &mut workbook)
}

#[cfg(feature = "office-support")]
/// Parse XLS file (legacy Excel format)
fn parse_xls(path: &Path) -> Result<ExtractionResult, String> {
    let mut workbook: Xls<_> =
        open_workbook(path).map_err(|e| format!("Failed to open workbook: {}", e))?;

    extract_xls_content(path, &mut workbook)
}

#[cfg(feature = "office-support")]
/// Parse CSV file
fn parse_csv(path: &Path) -> Result<ExtractionResult, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read CSV: {}", e))?;

    extract_csv_content(path, &content)
}

#[cfg(feature = "office-support")]
/// Extract content from XLSX workbook
fn extract_xlsx_content(
    path: &Path,
    workbook: &mut Xlsx<std::io::BufReader<std::fs::File>>,
) -> Result<ExtractionResult, String> {
    let sheet_names = workbook.sheet_names().to_vec();
    let mut all_content = String::new();
    let mut chunks = Vec::new();

    for (idx, sheet_name) in sheet_names.iter().enumerate() {
        if let Ok(range) = workbook.worksheet_range(sheet_name) {
            // Extract sheet content
            let sheet_content = range
                .rows()
                .map(|row| {
                    row.iter()
                        .map(|cell| cell.to_string())
                        .collect::<Vec<_>>()
                        .join("\t")
                })
                .collect::<Vec<_>>()
                .join("\n");

            if !sheet_content.is_empty() {
                all_content.push_str(&format!("=== Sheet: {} ===\n", sheet_name));
                all_content.push_str(&sheet_content);
                all_content.push_str("\n\n");

                // Create chunk for this sheet
                chunks.push(ContentChunk {
                    content: sheet_content,
                    chunk_type: "sheet".to_string(),
                    index: idx,
                });
            }
        }
    }

    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;

    Ok(ExtractionResult {
        document_type: "excel".to_string(),
        content: all_content,
        metadata: ExtractionMetadata {
            source_path: path.to_string_lossy().to_string(),
            format: "xlsx".to_string(),
            size_bytes: metadata.len(),
            extracted_at: chrono::Utc::now().to_rfc3339(),
        },
        chunks,
    })
}

#[cfg(feature = "office-support")]
/// Extract content from XLS workbook
fn extract_xls_content(
    path: &Path,
    workbook: &mut Xls<std::io::BufReader<std::fs::File>>,
) -> Result<ExtractionResult, String> {
    let sheet_names = workbook.sheet_names().to_vec();
    let mut all_content = String::new();
    let mut chunks = Vec::new();

    for (idx, sheet_name) in sheet_names.iter().enumerate() {
        if let Ok(range) = workbook.worksheet_range(sheet_name) {
            // Extract sheet content
            let sheet_content = range
                .rows()
                .map(|row| {
                    row.iter()
                        .map(|cell| cell.to_string())
                        .collect::<Vec<_>>()
                        .join("\t")
                })
                .collect::<Vec<_>>()
                .join("\n");

            if !sheet_content.is_empty() {
                all_content.push_str(&format!("=== Sheet: {} ===\n", sheet_name));
                all_content.push_str(&sheet_content);
                all_content.push_str("\n\n");

                chunks.push(ContentChunk {
                    content: sheet_content,
                    chunk_type: "sheet".to_string(),
                    index: idx,
                });
            }
        }
    }

    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;

    Ok(ExtractionResult {
        document_type: "excel".to_string(),
        content: all_content,
        metadata: ExtractionMetadata {
            source_path: path.to_string_lossy().to_string(),
            format: "xls".to_string(),
            size_bytes: metadata.len(),
            extracted_at: chrono::Utc::now().to_rfc3339(),
        },
        chunks,
    })
}

#[cfg(feature = "office-support")]
/// Extract content from CSV file
fn extract_csv_content(path: &Path, content: &str) -> Result<ExtractionResult, String> {
    let mut chunks = Vec::new();

    if !content.is_empty() {
        chunks.push(ContentChunk {
            content: content.to_string(),
            chunk_type: "csv".to_string(),
            index: 0,
        });
    }

    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;

    Ok(ExtractionResult {
        document_type: "excel".to_string(),
        content: content.to_string(),
        metadata: ExtractionMetadata {
            source_path: path.to_string_lossy().to_string(),
            format: "csv".to_string(),
            size_bytes: metadata.len(),
            extracted_at: chrono::Utc::now().to_rfc3339(),
        },
        chunks,
    })
}

/// Get sheet information without full parsing
#[cfg(feature = "office-support")]
pub fn get_sheet_info(path: &std::path::Path) -> Result<Vec<SheetInfo>, String> {
    use calamine::{open_workbook, Reader, Xlsx};

    let mut workbook: Xlsx<_> =
        open_workbook(path).map_err(|e| format!("Failed to open workbook: {}", e))?;

    let sheet_names = workbook.sheet_names().to_vec();
    let mut sheets = Vec::new();

    for sheet_name in sheet_names {
        if let Ok(range) = workbook.worksheet_range(&sheet_name) {
            let cols = range.rows().next().map(|r| r.len()).unwrap_or(0);
            sheets.push(SheetInfo {
                name: sheet_name,
                row_count: range.rows().count(),
                column_count: cols,
            });
        }
    }

    Ok(sheets)
}

#[cfg(not(feature = "office-support"))]
pub fn get_sheet_info(_path: &std::path::Path) -> Result<Vec<SheetInfo>, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
}

/// Read specific cell range from Excel file
#[cfg(feature = "office-support")]
pub fn read_range(
    path: &std::path::Path,
    sheet: &str,
    start_row: usize,
    end_row: usize,
    start_col: usize,
    end_col: usize,
) -> Result<Vec<Vec<String>>, String> {
    use calamine::{open_workbook, Reader, Xlsx};

    let mut workbook: Xlsx<_> =
        open_workbook(path).map_err(|e| format!("Failed to open workbook: {}", e))?;

    let range = workbook
        .worksheet_range(sheet)
        .map_err(|e| format!("Failed to get sheet: {}", e))?;

    let mut result = Vec::new();

    for (row_idx, row) in range.rows().enumerate() {
        if row_idx >= start_row && row_idx <= end_row {
            let mut row_data = Vec::new();
            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx >= start_col && col_idx <= end_col {
                    row_data.push(cell.to_string());
                }
            }
            result.push(row_data);
        }
    }

    Ok(result)
}

#[cfg(not(feature = "office-support"))]
pub fn read_range(
    _path: &std::path::Path,
    _sheet: &str,
    _start_row: usize,
    _end_row: usize,
    _start_col: usize,
    _end_col: usize,
) -> Result<Vec<Vec<String>>, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
}

#[cfg(test)]
mod tests {
    // Uses super::super::OfficeFormat directly below

    #[test]
    fn test_office_format() {
        assert_eq!(
            super::super::OfficeFormat::from_extension("xlsx"),
            super::super::OfficeFormat::Xlsx
        );
    }
}
