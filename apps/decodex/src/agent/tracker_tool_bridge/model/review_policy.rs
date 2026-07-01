use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use super::review::ReviewExecutionMode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewPolicyStopRequested {
	pub(crate) head_sha: String,
	pub(crate) issue_identifier: String,
	pub(crate) fingerprint: Option<String>,
	pub(crate) nonclean_rounds: Option<i64>,
	pub(crate) reason: ReviewPolicyStopReason,
	pub(crate) run_id: String,
}
impl Display for ReviewPolicyStopRequested {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self.reason {
			ReviewPolicyStopReason::Exhausted => write!(
				f,
				"Run `{}` for issue `{}` exhausted the runtime-owned review convergence budget at HEAD `{}` after {} non-clean rounds{}.",
				self.run_id,
				self.issue_identifier,
				self.head_sha,
				self.nonclean_rounds.unwrap_or_default(),
				self.fingerprint.as_ref().map_or_else(String::new, |fingerprint| format!(
					" for finding fingerprint `{fingerprint}`"
				))
			),
			ReviewPolicyStopReason::ArchitectureReviewRequired => write!(
				f,
				"Run `{}` for issue `{}` recorded `needs_architecture_review` at HEAD `{}` and now requires human architecture review.",
				self.run_id, self.issue_identifier, self.head_sha
			),
			ReviewPolicyStopReason::Blocked => write!(
				f,
				"Run `{}` for issue `{}` recorded `blocked` at HEAD `{}` and now requires human intervention.",
				self.run_id, self.issue_identifier, self.head_sha
			),
		}
	}
}

impl Error for ReviewPolicyStopRequested {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewPolicyStopReason {
	Exhausted,
	ArchitectureReviewRequired,
	Blocked,
}
impl ReviewPolicyStopReason {
	pub(crate) fn error_class(self) -> &'static str {
		match self {
			Self::Exhausted => "review_policy_exhausted",
			Self::ArchitectureReviewRequired => "architecture_review_required",
			Self::Blocked => "review_policy_blocked",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewPolicyState {
	pub(in crate::agent::tracker_tool_bridge) phase: ReviewPolicyPhase,
	pub(in crate::agent::tracker_tool_bridge) status: ReviewPolicyStatus,
	pub(in crate::agent::tracker_tool_bridge) head_sha: String,
	pub(in crate::agent::tracker_tool_bridge) nonclean_rounds: i64,
	pub(in crate::agent::tracker_tool_bridge) details_json: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::agent::tracker_tool_bridge) enum ReviewPolicyPhase {
	Handoff,
	Repair,
}
impl ReviewPolicyPhase {
	pub(in crate::agent::tracker_tool_bridge) fn as_str(self) -> &'static str {
		match self {
			Self::Handoff => "handoff",
			Self::Repair => "repair",
		}
	}

	pub(in crate::agent::tracker_tool_bridge) fn for_mode(
		mode: ReviewExecutionMode,
	) -> Option<Self> {
		match mode {
			ReviewExecutionMode::Handoff => Some(Self::Handoff),
			ReviewExecutionMode::Repair => Some(Self::Repair),
			ReviewExecutionMode::Closeout => None,
		}
	}

	pub(in crate::agent::tracker_tool_bridge) fn parse(
		value: &str,
	) -> std::result::Result<Self, String> {
		match value {
			"handoff" => Ok(Self::Handoff),
			"repair" => Ok(Self::Repair),
			other => Err(format!(
				"Unsupported review policy phase `{other}` in the run activity marker."
			)),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::agent::tracker_tool_bridge) enum ReviewPolicyStatus {
	Clean,
	Findings,
	NeedsArchitectureReview,
	Blocked,
}
impl ReviewPolicyStatus {
	pub(in crate::agent::tracker_tool_bridge) fn as_str(self) -> &'static str {
		match self {
			Self::Clean => "clean",
			Self::Findings => "findings",
			Self::NeedsArchitectureReview => "needs_architecture_review",
			Self::Blocked => "blocked",
		}
	}

	pub(in crate::agent::tracker_tool_bridge) fn parse(
		value: &str,
	) -> std::result::Result<Self, String> {
		match value {
			"clean" => Ok(Self::Clean),
			"findings" => Ok(Self::Findings),
			"needs_architecture_review" => Ok(Self::NeedsArchitectureReview),
			"blocked" => Ok(Self::Blocked),
			other => Err(format!(
				"`issue_review_checkpoint` status must be `clean`, `findings`, `needs_architecture_review`, or `blocked`, not `{other}`."
			)),
		}
	}
}
