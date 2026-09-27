//! Source-bound history-edit evidence. Applied history is not desktop draft acknowledgement.
use crate::{EntityId, WireText};
use serde::{Deserialize, Serialize};

/// Native edit lifecycle, separate from ordinary turn delivery.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptEditPhase {
	/// No edit is retained for this thread.
	Idle,
	/// A service-held native review can be explicitly confirmed.
	Review,
	/// A native write may have occurred; recover by reading, never by resubmission.
	Uncertain,
	/// Native prefix was observed; presentation recovery and draft handback are still required.
	Applied,
	/// The operation did not change history.
	Unchanged,
	/// The canonical draft was acknowledged through a separate handback path.
	Restored,
	/// Current native ownership or evidence is unavailable.
	Unavailable,
}

/// Bounded JSON content fragment. Offsets count UTF-8 bytes and never split a code point.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptEditEvidence {
	/// Exact service review, retained in the durable receipt after confirmation.
	pub review_token: WireText,
	/// Durable reservation identity, absent before confirmation.
	pub receipt_id: Option<i64>,
	/// First excluded native turn.
	pub before_turn_id: WireText,
	/// Exact original input item.
	pub item_id: WireText,
	/// Number of turns removed by this boundary, including the selected turn.
	pub removed_turns: u32,
	/// Total size of the complete canonical input JSON array.
	pub content_bytes: u64,
	/// First byte in this fragment.
	pub offset: u64,
	/// Canonical JSON bytes decoded as UTF-8; not user instructions to execute.
	pub fragment: String,
}

/// One exact work/thread status page. Reading it neither mutates history nor starts inference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptEditStatus {
	/// Local owner.
	pub work_id: EntityId,
	/// Exact native thread requested by the caller.
	pub thread_id: WireText,
	/// Current edit phase.
	pub phase: PromptEditPhase,
	/// Present only for retained review or receipt evidence.
	pub evidence: Option<PromptEditEvidence>,
}
impl PromptEditStatus {
	/// Validate bounded identities, phases and fragment offsets before assembling content.
	pub fn is_valid(&self) -> bool {
		if self.thread_id.as_str().is_empty() {
			return false;
		}
		let Some(e) = &self.evidence else {
			return matches!(self.phase, PromptEditPhase::Idle | PromptEditPhase::Unavailable);
		};
		!matches!(self.phase, PromptEditPhase::Idle | PromptEditPhase::Unavailable)
			&& !self.thread_id.as_str().is_empty()
			&& e.review_token.as_str().len() == 64
			&& e.review_token.as_str().bytes().all(|b| b.is_ascii_hexdigit())
			&& !e.before_turn_id.as_str().is_empty()
			&& !e.item_id.as_str().is_empty()
			&& e.removed_turns > 0
			&& e.content_bytes > 0
			&& e.content_bytes <= 8 * 1024 * 1024
			&& !e.fragment.is_empty()
			&& e.fragment.len() <= 64 * 1024
			&& e.offset
				.checked_add(e.fragment.len() as u64)
				.is_some_and(|end| end <= e.content_bytes)
			&& match self.phase {
				PromptEditPhase::Review => e.receipt_id.is_none(),
				_ => e.receipt_id.is_some_and(|id| id > 0),
			}
	}
}
