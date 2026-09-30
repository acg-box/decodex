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

/// Explicit native prefix selection. Both choices preserve the source conversation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptForkBoundary {
	/// Copy only turns before the reviewed input and restore that input as a draft.
	BeforeInput,
	/// Copy the reviewed completed turn and all earlier turns.
	AfterTurn,
}

/// Durable branch progress. Reading it never creates another branch.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptForkPhase {
	/// Native acceptance is not known. Do not replay the request.
	Uncertain,
	/// Native identity is saved; read-only history recovery remains.
	Acknowledged,
	/// Native prefix and local ownership are ready.
	Forked,
	/// Native request was rejected before creation.
	Rejected,
}

/// One branch receipt; canonical input remains in the existing paginated edit query.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptForkStatus {
	/// Original local owner.
	pub work_id: EntityId,
	/// Original native conversation.
	pub thread_id: WireText,
	/// Exact review identity.
	pub review_token: WireText,
	/// New local owner reserved before the native request.
	pub target_work_id: EntityId,
	/// Saved native acknowledgement, if received.
	pub target_thread_id: Option<WireText>,
	/// Explicit prefix choice.
	pub boundary: PromptForkBoundary,
	/// Durable progress.
	pub phase: PromptForkPhase,
	/// Before-input branches use the existing canonical draft handback.
	pub edit_receipt_id: Option<i64>,
}

/// Distinguish an absent attempt from an unavailable receipt store.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "receipt", rename_all = "snake_case")]
pub enum PromptForkResult {
	/// Read completed; no receipt means this exact review has not been reserved.
	Available(Option<PromptForkStatus>),
	/// Read failed; do not infer that no branch exists.
	Unavailable,
}

/// Desktop intent persists the branch choice and destination before submission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptForkIntent {
	/// Reserved local destination identity.
	pub target_work_id: EntityId,
	/// Explicit prefix choice.
	pub boundary: PromptForkBoundary,
}
