// Excel write module — create Excel files using rust_xlsxwriter
// Supports creating .xlsx files with data and basic formatting

use std::path::Path;

#[cfg(feature = "office-support")]
use rust_xlsxwriter::{Format, Workbook, XlsxColor};

/// Result of writing an Excel file
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WriteResult {
    pub path: String,
    pub sheets_created: usize,
    pub rows_written: usize,
    pub success: bool,
}

/// Data for a single cell
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CellData {
    pub value: CellValue,
    pub format: Option<CellFormat>,
}

/// Cell value types
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum CellValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Formula(String),
    Empty,
}

/// Cell formatting options
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CellFormat {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub color: Option<String>, // Hex color like "#FF0000"
    pub bg_color: Option<String>,
    pub number_format: Option<String>,
}

/// A row of data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RowData {
    pub cells: Vec<CellData>,
}

/// A sheet with data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SheetData {
    pub name: String,
    pub headers: Option<Vec<String>>,
    pub rows: Vec<RowData>,
}

/// Create an Excel file from sheet data
///
/// # Arguments
/// * `path` - Output file path
/// * `sheets` - Vector of sheets to create
#[cfg(feature = "office-support")]
pub fn write_excel(path: &Path, sheets: &[SheetData]) -> Result<WriteResult, String> {
    let path_str = path.to_string_lossy();
    let mut workbook = Workbook::new(&path_str);
    let mut total_rows = 0;

    for sheet_data in sheets {
        let worksheet = workbook.add_worksheet();

        // Write headers if present
        if let Some(headers) = &sheet_data.headers {
            let header_format = Format::new()
                .set_bold()
                .set_background_color(XlsxColor::Gray);

            for (col, header) in headers.iter().enumerate() {
                worksheet
                    .write_string(0, col as u16, header, &header_format)
                    .map_err(|e| format!("Failed to write header: {}", e))?;
            }
            total_rows += 1;
        }

        // Write data rows
        for (row_idx, row) in sheet_data.rows.iter().enumerate() {
            let data_row_idx = if sheet_data.headers.is_some() {
                row_idx + 1
            } else {
                row_idx
            };

            for (col_idx, cell) in row.cells.iter().enumerate() {
                write_cell(worksheet, data_row_idx as u32, col_idx as u16, cell)?;
            }
            total_rows += 1;
        }
    }

    workbook
        .close()
        .map_err(|e| format!("Failed to save workbook: {}", e))?;

    Ok(WriteResult {
        path: path.to_string_lossy().to_string(),
        sheets_created: sheets.len(),
        rows_written: total_rows,
        success: true,
    })
}

#[cfg(not(feature = "office-support"))]
pub fn write_excel(_path: &Path, _sheets: &[SheetData]) -> Result<WriteResult, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
}

/// Write a single cell to the worksheet
#[cfg(feature = "office-support")]
fn write_cell(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    cell: &CellData,
) -> Result<(), String> {
    // Build format if present
    let format = build_format(&cell.format);

    let default_format = Format::new();

    match &cell.value {
        CellValue::String(s) => {
            worksheet
                .write_string(row, col, s, &format.unwrap_or(default_format))
                .map_err(|e| format!("Failed to write string: {}", e))?;
        }
        CellValue::Number(n) => {
            worksheet
                .write_number(row, col, *n, &format.unwrap_or(default_format))
                .map_err(|e| format!("Failed to write number: {}", e))?;
        }
        CellValue::Boolean(b) => {
            worksheet
                .write_boolean(row, col, *b, &format.unwrap_or(default_format))
                .map_err(|e| format!("Failed to write boolean: {}", e))?;
        }
        CellValue::Formula(f) => {
            worksheet
                .write_formula(row, col, f, &format.unwrap_or(default_format))
                .map_err(|e| format!("Failed to write formula: {}", e))?;
        }
        CellValue::Empty => {
            worksheet
                .write_blank(row, col, &default_format)
                .map_err(|e| format!("Failed to write blank: {}", e))?;
        }
    }

    Ok(())
}

/// Build a Format from CellFormat options
#[cfg(feature = "office-support")]
fn build_format(cell_format: &Option<CellFormat>) -> Option<Format> {
    let cf = cell_format.as_ref()?;

    let mut format = Format::new();

    if cf.bold.unwrap_or(false) {
        format = format.set_bold();
    }

    if cf.italic.unwrap_or(false) {
        format = format.set_italic();
    }

    if let Some(color) = &cf.color {
        if let Some(c) = parse_hex_color(color) {
            format = format.set_font_color(c);
        }
    }

    if let Some(bg_color) = &cf.bg_color {
        if let Some(c) = parse_hex_color(bg_color) {
            format = format.set_background_color(c);
        }
    }

    if let Some(num_fmt) = &cf.number_format {
        format = format.set_num_format(num_fmt);
    }

    Some(format)
}

/// Parse hex color string to XlsxColor
#[cfg(feature = "office-support")]
fn parse_hex_color(hex: &str) -> Option<XlsxColor> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        // XlsxColor::RGB takes a single u32 - we pack RGB into one u32
        let r = u32::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u32::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u32::from_str_radix(&hex[4..6], 16).ok()?;
        let rgb = (r << 16) | (g << 8) | b;
        Some(XlsxColor::RGB(rgb))
    } else {
        None
    }
}

/// Simple function to write a 2D array to Excel
///
/// This is a convenience function for common use cases.
///
/// # Arguments
/// * `path` - Output file path
/// * `sheet_name` - Name of the first sheet (not used in v0.7)
/// * `data` - 2D array of strings (each inner vec is a row)
#[cfg(feature = "office-support")]
pub fn write_simple_excel(
    path: &Path,
    _sheet_name: &str,
    data: &[Vec<String>],
) -> Result<WriteResult, String> {
    let path_str = path.to_string_lossy();
    let mut workbook = Workbook::new(&path_str);

    let worksheet = workbook.add_worksheet();

    let default_format = Format::new();
    let mut row_count = 0;

    for (row_idx, row) in data.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            worksheet
                .write_string(row_idx as u32, col_idx as u16, cell, &default_format)
                .map_err(|e| format!("Failed to write cell: {}", e))?;
        }
        row_count += 1;
    }

    workbook
        .close()
        .map_err(|e| format!("Failed to save workbook: {}", e))?;

    Ok(WriteResult {
        path: path.to_string_lossy().to_string(),
        sheets_created: 1,
        rows_written: row_count,
        success: true,
    })
}

#[cfg(not(feature = "office-support"))]
pub fn write_simple_excel(
    _path: &Path,
    _sheet_name: &str,
    _data: &[Vec<String>],
) -> Result<WriteResult, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_hex_color() {
        assert!(parse_hex_color("#FF0000").is_some());
        assert!(parse_hex_color("FF0000").is_some());
        assert!(parse_hex_color("#XYZ").is_none());
        assert!(parse_hex_color("").is_none());
    }

    #[test]
    fn test_cell_value_serialization() {
        let cell = CellData {
            value: CellValue::String("test".to_string()),
            format: None,
        };
        let json = serde_json::to_string(&cell).unwrap();
        assert!(json.contains("test"));
    }

    #[test]
    fn test_sheet_data_serialization() {
        let sheet = SheetData {
            name: "Test".to_string(),
            headers: Some(vec!["Col1".to_string()]),
            rows: vec![RowData {
                cells: vec![CellData {
                    value: CellValue::Number(42.0),
                    format: None,
                }],
            }],
        };
        let json = serde_json::to_string(&sheet).unwrap();
        assert!(json.contains("Test"));
        assert!(json.contains("42"));
    }

    #[cfg(feature = "office-support")]
    #[test]
    fn test_write_simple_excel() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().with_extension("xlsx");

        let data = vec![
            vec!["Name".to_string(), "Age".to_string()],
            vec!["Alice".to_string(), "30".to_string()],
            vec!["Bob".to_string(), "25".to_string()],
        ];

        let result = write_simple_excel(&path, "People", &data);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.sheets_created, 1);
        assert!(path.exists());

        // Cleanup
        let _ = fs::remove_file(&path);
    }
}
