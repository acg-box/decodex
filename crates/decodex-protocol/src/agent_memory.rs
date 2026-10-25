//! Read-only, source-bound native memory pages.
use serde::{Deserialize, Serialize};

use crate::{EntityId, WireText};

/// Maximum UTF-8 bytes in a memory summary page.
pub const MEMORY_PAGE_BYTES: usize = 8 * 1024;
/// Maximum native summary assembled by the desktop viewer.
pub const MAX_MEMORY_BYTES: usize = 1024 * 1024;

/// Native memory observation. Empty does not distinguish disabled from missing memory.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentMemoryResult {
	/// The conversation is not loaded, the method is unsupported, or its source changed.
	Unavailable,
	/// Native memory use is disabled or the selected summary is absent, unreadable or blank.
	Empty {
		/// Exact local Agent.
		work_id: EntityId,
		/// Exact native conversation.
		thread_id: WireText,
	},
	/// One digest-bound portion of the native summary.
	Page {
		/// Exact local Agent.
		work_id: EntityId,
		/// Exact native conversation.
		thread_id: WireText,
		/// Identity of the source and complete summary.
		digest: String,
		/// UTF-8 byte offset of this page.
		offset: usize,
		/// Length of the complete native summary.
		total_bytes: usize,
		/// Original native memory text, without interpretation or model-generated additions.
		text: String,
		/// Next UTF-8 byte offset; absent on the final page.
		next_offset: Option<usize>,
	},
}
