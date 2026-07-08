use crate::{
	orchestrator::{
		self, PostReviewLaneClassification, PostReviewLaneDecision, PullRequestReadbackRootCause,
		PullRequestReviewState,
	},
	prelude::{Result, eyre},
	state::ReviewLifecycleRecord,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct PostReviewReadbackDegradation<'a> {
	pub(in crate::orchestrator) reason: &'a str,
	pub(in crate::orchestrator) root_cause: PullRequestReadbackRootCause,
	pub(in crate::orchestrator) pr_url: &'a str,
	pub(in crate::orchestrator) pr_head_sha: &'a str,
}
impl<'a> PostReviewReadbackDegradation<'a> {
	pub(in crate::orchestrator) fn tracker_issue_from_lifecycle(
		lifecycle_record: &'a ReviewLifecycleRecord,
	) -> Self {
		Self {
			reason: "tracker_issue_readback_degraded",
			root_cause: PullRequestReadbackRootCause::TrackerIssueReadbackFailed,
			pr_url: lifecycle_record.pr_url(),
			pr_head_sha: lifecycle_record.pr_head_oid(),
		}
	}

	pub(in crate::orchestrator) fn pull_request_state_from_lifecycle(
		lifecycle_record: &'a ReviewLifecycleRecord,
		root_cause: PullRequestReadbackRootCause,
	) -> Self {
		Self {
			reason: "pull_request_state_read_failed",
			root_cause,
			pr_url: lifecycle_record.pr_url(),
			pr_head_sha: lifecycle_record.pr_head_oid(),
		}
	}

	pub(in crate::orchestrator) fn wait_for_review_classification(
		self,
		review_state: Option<PullRequestReviewState>,
	) -> PostReviewLaneClassification {
		let (
			pr_head_sha,
			pr_state,
			review_decision,
			mergeable,
			check_state,
			unresolved_review_threads,
		) = match review_state {
			Some(review_state) => (
				Some(review_state.head_ref_oid),
				Some(review_state.state),
				review_state.review_decision,
				Some(review_state.mergeable),
				review_state.status_check_rollup_state,
				Some(review_state.unresolved_review_threads),
			),
			None => (Some(self.pr_head_sha.to_owned()), None, None, None, None, None),
		};

		PostReviewLaneClassification {
			decision: PostReviewLaneDecision::WaitForReview,
			reason: self.reason.to_owned(),
			pr_url: Some(self.pr_url.to_owned()),
			pr_head_sha,
			pr_state,
			review_decision,
			mergeable,
			check_state,
			unresolved_review_threads,
			readback_warning: Some(self.reason.to_owned()),
			readback_root_cause: Some(self.root_cause.as_str().to_owned()),
		}
	}
}

pub(in crate::orchestrator) struct PostReviewOrchestrationStatus {
	pub(in crate::orchestrator) action: PostReviewLifecycleAction,
	pub(in crate::orchestrator) request_acknowledged: bool,
	pub(in crate::orchestrator) review_result_arrived: bool,
	pub(in crate::orchestrator) strict_pass: bool,
	pub(in crate::orchestrator) clean_path_landing_gates_satisfied: bool,
	pub(in crate::orchestrator) landing_requires_agent_fallback: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) enum PostReviewLifecycleAction {
	StartReviewGateOrExternalReview,
	RequestExternalReview,
	WaitForExternalReviewAck,
	WaitForExternalReviewResult,
	WaitForLandingGates,
	RunReviewRepair,
	PollLandingReadback,
	RunCloseoutAdapter,
	RequestManualAttention,
}
impl PostReviewLifecycleAction {
	pub(in crate::orchestrator) fn parse(value: &str) -> Result<Self> {
		Ok(match value {
			"wait_for_runtime_review_gate_or_external_review" =>
				Self::StartReviewGateOrExternalReview,
			"request_external_review" => Self::RequestExternalReview,
			"wait_for_external_review_ack" => Self::WaitForExternalReviewAck,
			"wait_for_external_review_result" => Self::WaitForExternalReviewResult,
			"wait_for_landing_gates" => Self::WaitForLandingGates,
			"run_retained_review_repair_adapter" => Self::RunReviewRepair,
			"poll_landing_readback" => Self::PollLandingReadback,
			"run_retained_closeout_adapter" => Self::RunCloseoutAdapter,
			"request_manual_attention" => Self::RequestManualAttention,
			_ => eyre::bail!("Unknown post-review lifecycle action `{value}`."),
		})
	}
}
impl PostReviewOrchestrationStatus {
	pub(in crate::orchestrator) fn from_review_state(
		review_state: &PullRequestReviewState,
		lifecycle_record: &ReviewLifecycleRecord,
	) -> Result<Self> {
		let action = PostReviewLifecycleAction::parse(lifecycle_record.next_action())?;

		Ok(Self {
			action,
			request_acknowledged: orchestrator::request_comment_has_eyes(
				review_state,
				lifecycle_record,
			)
			.unwrap_or(false),
			review_result_arrived: orchestrator::external_review_result_arrived(
				review_state,
				lifecycle_record,
			),
			strict_pass: orchestrator::external_review_has_strict_pass_signals(
				review_state,
				lifecycle_record,
			),
			clean_path_landing_gates_satisfied:
				orchestrator::review_state_clean_path_landing_gates_satisfied(review_state),
			landing_requires_agent_fallback:
				orchestrator::review_state_landing_requires_agent_fallback(review_state),
		})
	}
}
