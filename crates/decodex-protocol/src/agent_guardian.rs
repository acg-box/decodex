//! Saved native review evidence and separate user-approval submission receipts.
use serde::{Deserialize, Serialize};

use decodex_core::MAX_NATIVE_MESSAGE_BYTES;

/// Maximum UTF-8 text bytes in one detail page, before JSON escaping.
pub const GUARDIAN_DETAIL_PAGE_BYTES: usize = 8 * 1_024;

/// Last observed native assessment, not execution status.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentGuardianStatus {
	/// No final result has been observed.
	InProgress,
	/// Native reviewer allowed the action.
	Approved,
	/// Native reviewer denied the action.
	Denied,
	/// Review deadline elapsed.
	TimedOut,
	/// Native reviewer stopped the assessment.
	Aborted,
}

/// Receipt for explicit user approval, independent of the native assessment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentGuardianSubmission {
	/// Submission was reserved, but no definitive response is saved. Never auto-resend.
	Pending,
	/// Native endpoint acknowledged submission; execution is not implied.
	Submitted,
	/// Native endpoint explicitly rejected submission.
	Rejected,
}

/// One bounded, displayable review bound to immutable saved evidence.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentGuardianReviewDto {
	/// Exact durable record identity.
	pub row_id: i64,
	/// Digest required when approving this displayed snapshot.
	pub digest: String,
	/// Readable action category.
	pub action_label: String,
	/// Last observed assessment.
	pub status: AgentGuardianStatus,
	/// Provider risk, absent while unknown.
	pub risk_level: Option<String>,
	/// Provider assessment of user authorization.
	pub user_authorization: Option<String>,
	/// Provider explanation; absence does not imply approval.
	pub rationale: Option<String>,
	/// Exact public action shown as quoted JSON; omitted when it cannot safely fit.
	pub action_json: Option<String>,
	/// Why action or rationale content could not be shown.
	pub details_unavailable: Option<String>,
	/// Complete details are available through the digest-bound detail reader.
	pub details_paged: bool,
	/// Whether the observation came from the currently retained native process.
	pub current_process: bool,
	/// Separate explicit approval receipt.
	pub submission: Option<AgentGuardianSubmission>,
	/// Exact user command owning this receipt, so an older rejection cannot clear a newer send.
	pub submission_key: Option<String>,
	/// Whether these details support an explicit approval request. The command
	/// rechecks native ownership and the latest turn before submission.
	pub can_approve: bool,
	/// Why a denied action is not eligible for approval.
	pub approval_unavailable: Option<String>,
}

/// Bounded durable reviews, with a cursor that never skips an omitted record.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentGuardianReviewsResult {
	/// Complete current page, newest first.
	Available {
		/// At most eight reviews within the page byte limit.
		reviews: Vec<AgentGuardianReviewDto>,
		/// Cursor for older reviews, independent of withheld display details.
		next_before: Option<i64>,
	},
	/// The work or its saved evidence cannot be read.
	Unavailable,
}

/// A page of complete saved action and rationale text. No action is truncated.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentGuardianDetailResult {
	/// One contiguous UTF-8 slice of the requested immutable observation.
	Available {
		/// Saved review row identity.
		row_id: i64,
		/// Digest of the complete observation.
		digest: String,
		/// Starting UTF-8 byte offset.
		offset: usize,
		/// Total UTF-8 bytes in the complete document.
		total_bytes: usize,
		/// Unmodified text at this offset.
		text: String,
		/// Next offset; absent at the document end.
		next_offset: Option<usize>,
	},
	/// Missing, changed, or undisplayable evidence. Refresh the review list.
	Unavailable,
}
impl AgentGuardianDetailResult {
	/// Verify response identity and contiguous bounds before showing a page.
	pub fn matches_request(&self, row: i64, expected_digest: &str, start: usize) -> bool {
		match self {
			Self::Unavailable => true,
			Self::Available { row_id, digest, offset, total_bytes, text, next_offset } => {
				let Some(end) = offset.checked_add(text.len()) else {
					return false;
				};

				*row_id == row
					&& digest == expected_digest
					&& *offset == start
					&& !text.is_empty()
					&& text.len() <= GUARDIAN_DETAIL_PAGE_BYTES
					&& end <= *total_bytes
					&& *total_bytes <= MAX_NATIVE_MESSAGE_BYTES
					&& *next_offset == (end < *total_bytes).then_some(end)
			},
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::AgentGuardianDetailResult;
	#[test]
	fn detail_pages_bind_identity_and_exact_continuation() {
		let page = AgentGuardianDetailResult::Available {
			row_id: 4,
			digest: "exact".into(),
			offset: 3,
			total_bytes: 9,
			text: "中文".into(),
			next_offset: None,
		};

		assert!(page.matches_request(4, "exact", 3));

		for (row, digest, offset) in [(5, "exact", 3), (4, "stale", 3), (4, "exact", 0)] {
			assert!(!page.matches_request(row, digest, offset));
		}
		for next_offset in [Some(3), Some(8), Some(9), Some(10)] {
			let mut bad = page.clone();

			if let AgentGuardianDetailResult::Available { next_offset: next, .. } = &mut bad {
				*next = next_offset;
			}

			assert!(!bad.matches_request(4, "exact", 3));
		}
	}
}
