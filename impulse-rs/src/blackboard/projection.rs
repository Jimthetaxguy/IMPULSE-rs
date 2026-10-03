//! Demand paging over a stored payload: a reader asks for one window of an
//! entry (optionally after selecting a JSON sub-value) instead of the whole
//! thing, so fetching a large result never re-floods the context it was
//! moved out of.

use serde::{Deserialize, Serialize};

/// Largest window one fetch returns. Matches the 8,192-byte read limit the
/// PRD sets for workspace reads.
pub const MAX_PAGE_BYTES: usize = 8 * 1024;

/// What part of an entry to return.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Projection {
    /// RFC 6901 pointer into a JSON payload, e.g. `/results/0/path`. Applied
    /// before paging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_pointer: Option<String>,
    /// Byte offset of the window. Snapped back to a character boundary.
    pub offset: usize,
    /// Window size in bytes, clamped to [`MAX_PAGE_BYTES`]. `None` means the
    /// largest window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// One window of an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    /// The window's text.
    pub content: String,
    /// Size of the projected text (after any JSON pointer), in bytes.
    pub total_bytes: usize,
    /// Byte offset the window actually starts at.
    pub offset: usize,
    /// Where the next window starts, or `None` when this one reaches the end.
    pub next_offset: Option<usize>,
}

/// Why a projection could not be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectionError {
    #[error(
        "payload is not UTF-8 text ({bytes} bytes of {content_type}); it cannot be paged as text"
    )]
    NotText { bytes: usize, content_type: String },
    #[error("json_pointer was given but the payload is not valid JSON")]
    NotJson,
    #[error("json_pointer {pointer:?} does not select anything in the payload")]
    PointerMissing { pointer: String },
    #[error("offset {offset} is past the end of the {total}-byte projected text")]
    OffsetPastEnd { offset: usize, total: usize },
    #[error("limit must be at least 1")]
    ZeroLimit,
}

/// Applies `projection` to `payload`.
pub fn project(
    payload: &[u8],
    content_type: &str,
    projection: &Projection,
) -> Result<Page, ProjectionError> {
    let text = std::str::from_utf8(payload).map_err(|_| ProjectionError::NotText {
        bytes: payload.len(),
        content_type: content_type.to_string(),
    })?;
    let selected;
    let text = match projection.json_pointer.as_deref() {
        None => text,
        Some(pointer) => {
            let value: serde_json::Value =
                serde_json::from_str(text).map_err(|_| ProjectionError::NotJson)?;
            let found = value
                .pointer(pointer)
                .ok_or_else(|| ProjectionError::PointerMissing {
                    pointer: pointer.to_string(),
                })?;
            selected = match found {
                serde_json::Value::String(text) => text.clone(),
                other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
            };
            selected.as_str()
        }
    };
    page(text, projection.offset, projection.limit)
}

/// One window of `text`, with both ends on character boundaries.
pub fn page(text: &str, offset: usize, limit: Option<usize>) -> Result<Page, ProjectionError> {
    let total = text.len();
    if limit == Some(0) {
        return Err(ProjectionError::ZeroLimit);
    }
    if offset > total || (offset == total && total > 0) {
        return Err(ProjectionError::OffsetPastEnd { offset, total });
    }
    let start = floor_char_boundary(text, offset);
    let limit = limit.unwrap_or(MAX_PAGE_BYTES).min(MAX_PAGE_BYTES);
    let mut end = floor_char_boundary(text, start.saturating_add(limit).min(total));
    if end == start && start < total {
        // A limit smaller than one multi-byte character still makes progress.
        end = ceil_char_boundary(text, start + 1);
    }
    Ok(Page {
        content: text[start..end].to_string(),
        total_bytes: total,
        offset: start,
        next_offset: (end < total).then_some(end),
    })
}

/// The first `max_bytes` of `text`, cut on a character boundary.
pub fn preview(text: &str, max_bytes: usize) -> &str {
    &text[..floor_char_boundary(text, max_bytes.min(text.len()))]
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_whole_short_text_has_no_next_offset() {
        let page = page("hello", 0, None).expect("page");
        assert_eq!(page.content, "hello");
        assert_eq!(page.total_bytes, 5);
        assert_eq!(page.next_offset, None);
    }

    #[test]
    fn test_page_walks_a_long_text_in_windows() {
        let text = "abcdefghij";
        let first = page(text, 0, Some(4)).expect("first");
        assert_eq!(first.content, "abcd");
        assert_eq!(first.next_offset, Some(4));
        let second = page(text, 4, Some(4)).expect("second");
        assert_eq!(second.content, "efgh");
        let last = page(text, 8, Some(4)).expect("last");
        assert_eq!(last.content, "ij");
        assert_eq!(last.next_offset, None);
    }

    #[test]
    fn test_page_clamps_limit_to_max_page() {
        let text = "x".repeat(MAX_PAGE_BYTES * 2);
        let page = page(&text, 0, Some(usize::MAX)).expect("page");
        assert_eq!(page.content.len(), MAX_PAGE_BYTES);
        assert_eq!(page.next_offset, Some(MAX_PAGE_BYTES));
    }

    #[test]
    fn test_page_snaps_offsets_to_char_boundaries() {
        let text = "aéb"; // 'é' is two bytes at 1..3
        let page = page(text, 2, Some(1)).expect("page");
        assert_eq!(page.offset, 1, "offset inside 'é' snaps back");
        assert_eq!(page.content, "é", "tiny limit still returns one char");
        assert_eq!(page.next_offset, Some(3));
    }

    #[test]
    fn test_page_rejects_offset_past_end_and_zero_limit() {
        assert!(matches!(
            page("abc", 3, None),
            Err(ProjectionError::OffsetPastEnd { .. })
        ));
        assert!(matches!(
            page("abc", 0, Some(0)),
            Err(ProjectionError::ZeroLimit)
        ));
        let empty = page("", 0, None).expect("empty text pages to empty");
        assert_eq!(empty.content, "");
    }

    #[test]
    fn test_project_json_pointer_selects_sub_value() {
        let payload = br#"{"results":[{"path":"src/lib.rs","line":7}]}"#;
        let projection = Projection {
            json_pointer: Some("/results/0/path".to_string()),
            ..Projection::default()
        };
        let page = project(payload, "application/json", &projection).expect("project");
        assert_eq!(page.content, "src/lib.rs");
        let projection = Projection {
            json_pointer: Some("/results/0".to_string()),
            ..Projection::default()
        };
        let page = project(payload, "application/json", &projection).expect("project");
        assert!(page.content.contains("\"line\": 7"));
    }

    #[test]
    fn test_project_errors_name_the_problem() {
        let pointer = Projection {
            json_pointer: Some("/missing".to_string()),
            ..Projection::default()
        };
        assert!(matches!(
            project(b"not json", "text/plain", &pointer),
            Err(ProjectionError::NotJson)
        ));
        assert!(matches!(
            project(b"{}", "application/json", &pointer),
            Err(ProjectionError::PointerMissing { .. })
        ));
        let err = project(
            &[0xff, 0xfe],
            "application/octet-stream",
            &Projection::default(),
        )
        .expect_err("binary");
        assert!(err.to_string().contains("not UTF-8"));
    }

    #[test]
    fn test_projection_round_trips_and_rejects_unknown_fields() {
        let original = Projection {
            json_pointer: Some("/a".to_string()),
            offset: 10,
            limit: Some(20),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: Projection = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
        assert!(serde_json::from_str::<Projection>(r#"{"ofset":1}"#).is_err());
    }

    #[test]
    fn test_page_round_trips_through_json() {
        let original = page("hello world", 6, Some(3)).expect("page");
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: Page = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_preview_cuts_on_char_boundary() {
        assert_eq!(preview("aéb", 2), "a");
        assert_eq!(preview("abc", 10), "abc");
    }

    #[test]
    fn test_projection_error_display() {
        assert!(ProjectionError::ZeroLimit
            .to_string()
            .contains("at least 1"));
        assert!(ProjectionError::OffsetPastEnd {
            offset: 9,
            total: 3
        }
        .to_string()
        .contains("offset 9"));
    }

    proptest::proptest! {
        #[test]
        fn test_paging_reassembles_the_original(text in "\\PC{0,200}", limit in 1usize..40) {
            let mut offset = 0;
            let mut rebuilt = String::new();
            if !text.is_empty() {
                loop {
                    let window = page(&text, offset, Some(limit)).expect("page");
                    proptest::prop_assert_eq!(window.offset, offset);
                    rebuilt.push_str(&window.content);
                    match window.next_offset {
                        Some(next) => offset = next,
                        None => break,
                    }
                }
            }
            proptest::prop_assert_eq!(rebuilt, text);
        }
    }
}
