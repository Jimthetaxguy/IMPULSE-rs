// Excel module - parse and extract data from Excel files
// Cells are streamed through calamine's cell reader by `office::bounded`;
// calamine's dense-grid reader is never used.

#[cfg(not(feature = "office-support"))]
use crate::office::ExtractionResult;
use crate::office::SheetInfo;
#[cfg(feature = "office-support")]
use crate::office::{bounded, ExtractionResult};

/// Parse an Excel workbook (`.xlsx`) or CSV file under the bounds in
/// `office::bounded`. Legacy `.xls` is refused.
#[cfg(feature = "office-support")]
pub fn parse_excel(path: &std::path::Path) -> Result<ExtractionResult, String> {
    bounded::check_extension(path, &bounded::path_label(path), &["xlsx", "csv"])
        .map_err(|e| format!("{e:#}"))?;
    super::read_bounded(path)
}

#[cfg(not(feature = "office-support"))]
pub fn parse_excel(_path: &std::path::Path) -> Result<ExtractionResult, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
}

/// Lists a workbook's sheets with the extent of their non-empty cells,
/// which are streamed rather than laid out in a grid, under the file,
/// container and cell bounds of `office::bounded`. Chart and dialog sheets
/// hold no cells and are listed with no rows or columns.
#[cfg(feature = "office-support")]
pub fn get_sheet_info(path: &std::path::Path) -> Result<Vec<SheetInfo>, String> {
    let label = bounded::path_label(path);
    bounded::contain_panics(&label, || sheet_info(path, &label, bounded::MAX_CELLS))
        .map_err(|e| format!("{e:#}"))
}

/// [`get_sheet_info`] with an explicit cell cap; the test seam.
#[cfg(feature = "office-support")]
fn sheet_info(
    path: &std::path::Path,
    label: &str,
    max_cells: u64,
) -> anyhow::Result<Vec<SheetInfo>> {
    use calamine::{open_workbook, DataRef, Reader, Xlsx};

    bounded::check_extension(path, label, &["xlsx"])?;
    bounded::check_source_file(path, label, bounded::MAX_DOCUMENT_BYTES)?;
    bounded::preflight_container(path, label)?;
    let mut workbook: Xlsx<_> =
        open_workbook(path).map_err(|e| anyhow::anyhow!("{label} could not be parsed: {e}"))?;
    let names = workbook.sheet_names().to_vec();
    bounded::check_sheet_count(label, names.len())?;
    let mut sheets = Vec::with_capacity(names.len());
    let mut cells_total: u64 = 0;
    for name in names {
        // The first and last row, and column, holding a non-empty cell.
        let mut rows: Option<(u32, u32)> = None;
        let mut columns: Option<(u32, u32)> = None;
        match workbook.worksheet_cells_reader(&name) {
            Ok(mut reader) => {
                while let Some(cell) = reader.next_cell().map_err(|e| {
                    anyhow::anyhow!("{label} could not be parsed: sheet '{name}': {e}")
                })? {
                    if matches!(cell.get_value(), DataRef::Empty) {
                        continue;
                    }
                    cells_total += 1;
                    if cells_total > max_cells {
                        anyhow::bail!(
                            "{label} has more than {max_cells} non-empty cells, over the limit"
                        );
                    }
                    let (row, column) = cell.get_position();
                    rows = Some(
                        rows.map_or((row, row), |(first, last)| (first.min(row), last.max(row))),
                    );
                    columns = Some(columns.map_or((column, column), |(first, last)| {
                        (first.min(column), last.max(column))
                    }));
                }
            }
            Err(calamine::XlsxError::NotAWorksheet(_)) => {}
            Err(e) => anyhow::bail!("{label} could not be parsed: sheet '{name}': {e}"),
        }
        let extent =
            |span: Option<(u32, u32)>| span.map_or(0, |(first, last)| (last - first) as usize + 1);
        sheets.push(SheetInfo {
            name,
            row_count: extent(rows),
            column_count: extent(columns),
        });
    }
    Ok(sheets)
}

#[cfg(not(feature = "office-support"))]
pub fn get_sheet_info(_path: &std::path::Path) -> Result<Vec<SheetInfo>, String> {
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

    #[cfg(feature = "office-support")]
    mod bounded_reads {
        use crate::office::test_fixtures::*;

        /// Two cells far apart cost a row marker and a column marker. The
        /// dense grid this replaced rendered about 900,000 tab-separated
        /// cells for this sheet, and billions for two cells at opposite
        /// corners of a sheet.
        #[test]
        fn test_parse_excel_streams_far_apart_cells() {
            let dir = tempfile::tempdir().unwrap();
            let path = write_far_apart_workbook(dir.path());
            let result = super::super::parse_excel(&path).unwrap();
            assert_eq!(
                result.content,
                "=== Sheet: Data ===\nstart\n[2999 empty rows]\n[300 empty columns]\tend\n\n"
            );
        }

        /// Extents come from the streamed cells: from the first non-empty
        /// row and column to the last, and none for an empty sheet.
        #[test]
        fn test_get_sheet_info_reports_extents_without_a_grid() {
            let dir = tempfile::tempdir().unwrap();
            let path = write_far_apart_workbook(dir.path());
            let sheets = super::super::get_sheet_info(&path).unwrap();
            let summary: Vec<_> = sheets
                .iter()
                .map(|s| (s.name.as_str(), s.row_count, s.column_count))
                .collect();
            assert_eq!(summary, [("Data", 3001, 301), ("Empty", 0, 0)]);
        }

        /// The sheet listing preflights the container before calamine opens
        /// it, and refuses legacy `.xls`.
        #[test]
        fn test_get_sheet_info_preflights_the_container_and_refuses_xls() {
            let dir = tempfile::tempdir().unwrap();
            let broken = dir.path().join("broken.xlsx");
            std::fs::write(&broken, b"not a zip").unwrap();
            let err = super::super::get_sheet_info(&broken).unwrap_err();
            assert!(err.contains("not a valid xlsx container"), "{err}");

            let xls = dir.path().join("legacy.xls");
            std::fs::write(&xls, b"not a zip").unwrap();
            let err = super::super::get_sheet_info(&xls).unwrap_err();
            assert!(err.contains("legacy .xls"), "{err}");
        }

        /// The listing refuses a workbook over the sheet cap before it
        /// opens any sheet.
        #[test]
        fn test_get_sheet_info_refuses_more_sheets_than_the_cap() {
            let dir = tempfile::tempdir().unwrap();
            let sheets = crate::office::bounded::MAX_SHEETS + 1;
            let path = write_workbook_listing_sheets(dir.path(), sheets);
            let err = super::super::get_sheet_info(&path).unwrap_err();
            assert!(
                err.ends_with("has 4097 sheets, over the 4096-sheet limit"),
                "{err}"
            );
        }

        /// The listing stops at the cell cap instead of counting on.
        #[test]
        fn test_sheet_info_refuses_more_cells_than_the_cap() {
            let dir = tempfile::tempdir().unwrap();
            let path = write_far_apart_workbook(dir.path());
            assert_eq!(
                super::super::sheet_info(&path, "'far'", 2).unwrap().len(),
                2
            );
            let err = super::super::sheet_info(&path, "'far'", 1).unwrap_err();
            assert_eq!(
                err.to_string(),
                "'far' has more than 1 non-empty cells, over the limit"
            );
        }
    }
}
