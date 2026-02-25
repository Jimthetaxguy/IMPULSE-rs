//! Excel write tool — create Excel files using rust_xlsxwriter

use async_trait::async_trait;
use std::path::PathBuf;

use crate::office::excel_write::{self, CellData, CellFormat, CellValue, RowData, SheetData};
use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Write Excel files with data, formatting, and multiple sheets.
///
/// Supports: .xlsx files with:
/// - Multiple sheets
/// - Headers with formatting
/// - Cell data (strings, numbers, booleans, formulas)
/// - Cell formatting (bold, colors, number formats)
/// - Column alignment
pub struct ExcelWriteTool;

#[async_trait]
impl DynamicTool for ExcelWriteTool {
    fn id(&self) -> &str {
        "excel_write"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "excel_write".into(),
            name: "Excel Write".into(),
            description: "Create Excel .xlsx files with data, formatting, and multiple sheets".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Document,
            params: vec![
                ToolParam {
                    name: "path".into(),
                    description: "Output path for the Excel file (must end in .xlsx)".into(),
                    param_type: ParamType::FilePath,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "sheets".into(),
                    description: "Array of sheets to create. Each sheet has: name (string), headers (optional string array), rows (array of cell arrays)".into(),
                    param_type: ParamType::Json,
                    required: true,
                    default: None,
                },
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        // Validate path
        let path = params.get("path").and_then(|v| v.as_str());
        match path {
            Some(p) if !p.trim().is_empty() => {
                let path = PathBuf::from(p);
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext.to_lowercase() != "xlsx" {
                    return Err(ToolError::InvalidParams(
                        "Output path must end in .xlsx".into(),
                    ));
                }
                Ok(())
            }
            _ => Err(ToolError::InvalidParams(
                "missing or empty 'path' string".into(),
            )),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams("missing 'path' string parameter".into()))?;
        let path = PathBuf::from(path_str);

        // Parse sheets from JSON
        let sheets_json = params["sheets"]
            .as_array()
            .ok_or_else(|| ToolError::InvalidParams("'sheets' must be an array".into()))?;

        let mut sheets = Vec::new();

        for sheet_json in sheets_json {
            let sheet_obj = sheet_json
                .as_object()
                .ok_or_else(|| ToolError::InvalidParams("each sheet must be an object".into()))?;

            let name = sheet_obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Sheet")
                .to_string();

            // Parse headers
            let headers = sheet_obj
                .get("headers")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                });

            // Parse rows
            let mut rows = Vec::new();
            if let Some(rows_json) = sheet_obj.get("rows").and_then(|v| v.as_array()) {
                for row_json in rows_json {
                    let row_arr = row_json.as_array().ok_or_else(|| {
                        ToolError::InvalidParams("each row must be an array".into())
                    })?;

                    let mut cells = Vec::new();
                    for cell_json in row_arr {
                        let cell = parse_cell_json(cell_json)?;
                        cells.push(cell);
                    }
                    rows.push(RowData { cells });
                }
            }

            sheets.push(SheetData {
                name,
                headers,
                rows,
            });
        }

        // Write the Excel file
        let result =
            excel_write::write_excel(&path, &sheets).map_err(ToolError::ExecutionFailed)?;

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "sheets_created".to_string(),
            result.sheets_created.to_string(),
        );
        metadata.insert("rows_written".to_string(), result.rows_written.to_string());
        metadata.insert("path".to_string(), result.path.clone());

        Ok(ToolResult {
            output: serde_json::json!({
                "success": result.success,
                "path": result.path,
                "sheets_created": result.sheets_created,
                "rows_written": result.rows_written,
            }),
            artifacts: vec![],
            metadata,
        })
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemWrite]
    }
}

/// Parse a cell from JSON
fn parse_cell_json(json: &serde_json::Value) -> Result<CellData, ToolError> {
    // Handle different JSON formats:
    // 1. Simple string/number: "value" or 42
    // 2. Object with value: {"value": "text"}
    // 3. Object with value and format: {"value": "text", "format": {"bold": true}}

    let (value, format) = if json.is_string() || json.is_number() || json.is_boolean() {
        let value = match json {
            serde_json::Value::String(s) => CellValue::String(s.clone()),
            serde_json::Value::Number(n) => CellValue::Number(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::Bool(b) => CellValue::Boolean(*b),
            _ => return Err(ToolError::InvalidParams("Invalid cell value type".into())),
        };
        (value, None)
    } else if let Some(obj) = json.as_object() {
        let value = if let Some(v) = obj.get("value") {
            match v {
                serde_json::Value::String(s) => CellValue::String(s.clone()),
                serde_json::Value::Number(n) => CellValue::Number(n.as_f64().unwrap_or(0.0)),
                serde_json::Value::Bool(b) => CellValue::Boolean(*b),
                serde_json::Value::Null => CellValue::Empty,
                _ => return Err(ToolError::InvalidParams("Invalid cell value type".into())),
            }
        } else {
            return Err(ToolError::InvalidParams(
                "cell must have 'value' field".into(),
            ));
        };

        let format = if let Some(fmt_json) = obj.get("format").and_then(|v| v.as_object()) {
            let cf = CellFormat {
                bold: fmt_json.get("bold").and_then(|v| v.as_bool()),
                italic: fmt_json.get("italic").and_then(|v| v.as_bool()),
                color: fmt_json
                    .get("color")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                bg_color: fmt_json
                    .get("bg_color")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                number_format: fmt_json
                    .get("number_format")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            };
            // Only include Some if at least one option is set
            if cf.bold.is_none()
                && cf.italic.is_none()
                && cf.color.is_none()
                && cf.bg_color.is_none()
                && cf.number_format.is_none()
            {
                None
            } else {
                Some(cf)
            }
        } else {
            None
        };

        (value, format)
    } else {
        return Err(ToolError::InvalidParams(
            "cell must be a string, number, boolean, or object".into(),
        ));
    };

    Ok(CellData { value, format })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = ExcelWriteTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "excel_write");
        assert_eq!(desc.category, ToolCategory::Document);
    }

    #[test]
    fn test_validate_xlsx() {
        let tool = ExcelWriteTool;
        let params = serde_json::json!({"path": "output.xlsx", "sheets": []});
        assert!(tool.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_unsupported_extension() {
        let tool = ExcelWriteTool;
        let params = serde_json::json!({"path": "output.csv", "sheets": []});
        assert!(tool.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_missing_path() {
        let tool = ExcelWriteTool;
        let params = serde_json::json!({"sheets": []});
        assert!(tool.validate_params(&params).is_err());
    }

    #[test]
    fn test_parse_cell_string() {
        let cell = parse_cell_json(&serde_json::json!("hello")).unwrap();
        match cell.value {
            CellValue::String(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected String"),
        }
    }

    #[test]
    fn test_parse_cell_number() {
        let cell = parse_cell_json(&serde_json::json!(42.5)).unwrap();
        match cell.value {
            CellValue::Number(n) => assert_eq!(n, 42.5),
            _ => panic!("Expected Number"),
        }
    }

    #[test]
    fn test_parse_cell_object() {
        let cell = parse_cell_json(&serde_json::json!({
            "value": "test",
            "format": {"bold": true}
        }))
        .unwrap();
        match cell.value {
            CellValue::String(s) => assert_eq!(s, "test"),
            _ => panic!("Expected String"),
        }
        assert!(cell.format.is_some());
    }

    #[tokio::test]
    async fn test_execute_missing_path() {
        let tool = ExcelWriteTool;
        let ctx = ToolContext::default();
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }
}
