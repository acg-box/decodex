//! Exact selected Work details in digest-bound UTF-8 pages.
use serde::{Deserialize, Serialize};

/// Maximum retained selected Work response assembled by the client.
pub const MAX_WORK_DETAIL_BYTES: usize = 32 * 1024 * 1024;
/// One bounded transport page, before JSON escaping.
pub const WORK_DETAIL_PAGE_BYTES: usize = 8 * 1024;

/// A selected immutable view; callers must restart the read if its digest changes.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkDetailResult {
	/// One complete contiguous slice.
	Page {
		/// SHA-256 of the complete selected view.
		digest: String,
		/// UTF-8 byte offset.
		offset: usize,
		/// Complete encoded view length.
		total_bytes: usize,
		/// Exact JSON text at this offset.
		text: String,
		/// Next byte offset, absent at the end.
		next_offset: Option<usize>,
	},
	/// Missing, changed or unavailable selected evidence. Read from offset zero again.
	Unavailable,
}
impl WorkDetailResult {
	/// Validate exact paging and source digest before appending text.
	pub fn matches_request(&self, expected: Option<&str>, start: usize) -> bool {
		match self {
			Self::Unavailable => true,
			Self::Page { digest, offset, total_bytes, text, next_offset } => {
				let Some(end) = offset.checked_add(text.len()) else { return false };
				digest.len() == 64
					&& digest.bytes().all(|b| b.is_ascii_hexdigit())
					&& expected.is_none_or(|d| d == digest)
					&& *offset == start
					&& !text.is_empty()
					&& text.len() <= WORK_DETAIL_PAGE_BYTES
					&& end <= *total_bytes
					&& *total_bytes <= MAX_WORK_DETAIL_BYTES
					&& *next_offset == (end < *total_bytes).then_some(end)
			},
		}
	}
}
