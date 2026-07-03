use serde::{Deserialize, Serialize};

use crate::{
	orchestrator::{
		PostReviewLaneDecision, types::operator_status::loop_status::OperatorLoopStatus,
	},
	state::{ReviewHandoffMarker, WorktreeMapping},
	tracker::TrackerIssue,
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorPostReviewLaneStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) issue_state: String,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: String,
	pub(crate) classification: String,
	pub(crate) reason: String,
	pub(crate) pr_url: Option<String>,
	pub(crate) pr_head_sha: Option<String>,
	pub(crate) pr_state: Option<String>,
	pub(crate) review_decision: Option<String>,
	pub(crate) mergeable: Option<String>,
	pub(crate) check_state: Option<String>,
	pub(crate) unresolved_review_threads: Option<usize>,
	pub(crate) shadowed_by_current_lane: bool,
	pub(crate) readback_warning: Option<String>,
	pub(crate) readback_root_cause: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) loop_status: Option<OperatorLoopStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PostReviewLaneSnapshot {
	pub(crate) issue: TrackerIssue,
	pub(crate) worktree: WorktreeMapping,
	pub(crate) review_handoff: Option<ReviewHandoffMarker>,
	pub(crate) local_branch_name: Option<String>,
	pub(crate) local_head_oid: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PostReviewLaneClassification {
	pub(crate) decision: PostReviewLaneDecision,
	pub(crate) reason: String,
	pub(crate) pr_url: Option<String>,
	pub(crate) pr_head_sha: Option<String>,
	pub(crate) pr_state: Option<String>,
	pub(crate) review_decision: Option<String>,
	pub(crate) mergeable: Option<String>,
	pub(crate) check_state: Option<String>,
	pub(crate) unresolved_review_threads: Option<usize>,
	pub(crate) readback_warning: Option<String>,
	pub(crate) readback_root_cause: Option<String>,
}

pub(crate) struct RetainedReviewLaneBlocked {
	pub(crate) issue: TrackerIssue,
	pub(crate) worktree: WorktreeMapping,
	pub(crate) run_identity: RetainedReviewRunIdentity,
	pub(crate) reason: String,
}

pub(crate) struct RetainedReviewRunIdentity {
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
}
