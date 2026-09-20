//! Saved native review evidence and separate user-approval submission receipts.
use serde::{Deserialize, Serialize};

/// Last observed native assessment, not execution status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefGuardianStatus {
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
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefGuardianSubmission {
	/// Submission was reserved, but no definitive response is saved. Never auto-resend.
	Pending,
	/// Native endpoint acknowledged submission; execution is not implied.
	Submitted,
	/// Native endpoint explicitly rejected submission.
	Rejected,
}

/// One bounded, displayable review bound to immutable saved evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefGuardianReviewDto {
	/// Exact durable record identity.
	pub row_id: i64,
	/// Digest required when approving this displayed snapshot.
	pub digest: String,
	/// Readable action category.
	pub action_label: String,
	/// Last observed assessment.
	pub status: ChiefGuardianStatus,
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
	/// Whether the observation came from the currently retained native process.
	pub current_process: bool,
	/// Separate explicit approval receipt.
	pub submission: Option<ChiefGuardianSubmission>,
	/// Exact user command owning this receipt, so an older rejection cannot clear a newer send.
	pub submission_key: Option<String>,
	/// Whether these details support an explicit approval request. The command
	/// rechecks native ownership and the latest turn before submission.
	pub can_approve: bool,
	/// Why a denied action is not eligible for approval.
	pub approval_unavailable: Option<String>,
}

/// Bounded durable reviews, with a cursor that never skips an omitted record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefGuardianReviewsResult {
	/// Complete current page, newest first.
	Available {
		/// At most eight reviews within the page byte limit.
		reviews: Vec<ChiefGuardianReviewDto>,
		/// Cursor for older reviews, independent of withheld display details.
		next_before: Option<i64>,
	},
	/// The work or its saved evidence cannot be read.
	Unavailable,
}
