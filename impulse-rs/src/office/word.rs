// Word module - parse and extract data from Word documents
// `word/document.xml` is streamed through quick-xml by `office::bounded`;
// the docx object tree is never built.

#[cfg(feature = "office-support")]
use crate::office::bounded;
use crate::office::ExtractionResult;

/// Parse a Word document (`.docx`) under the bounds in `office::bounded`:
/// one line per non-empty paragraph and one per table row, its cells
/// separated by tabs.
#[cfg(feature = "office-support")]
pub fn parse_word(path: &std::path::Path) -> Result<ExtractionResult, String> {
    bounded::check_extension(path, &bounded::path_label(path), &["docx"])
        .map_err(|e| format!("{e:#}"))?;
    super::read_bounded(path)
}

#[cfg(not(feature = "office-support"))]
pub fn parse_word(_path: &std::path::Path) -> Result<ExtractionResult, String> {
    Err("Office support not enabled. Build with --features office-support".to_string())
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

    #[cfg(feature = "office-support")]
    mod bounded_reads {
        use crate::office::test_fixtures::*;

        /// A table row is one line of tab-separated cells; the docx object
        /// tree this replaced rendered every table as `[Table]`.
        #[test]
        fn test_parse_word_renders_table_rows() {
            let dir = tempfile::tempdir().unwrap();
            let path = write_docx_with_table(dir.path());
            let result = super::super::parse_word(&path).unwrap();
            assert_eq!(result.content, "Intro\nA\tB\nOutro\n");
        }

        /// `parse_word` reads only `.docx`; it used to hand any file to the
        /// docx parser.
        #[test]
        fn test_parse_word_refuses_other_extensions() {
            let dir = tempfile::tempdir().unwrap();
            let path = write_far_apart_workbook(dir.path());
            let err = super::super::parse_word(&path).unwrap_err();
            assert!(err.contains("unsupported extension 'xlsx'"), "{err}");
        }
    }
}
