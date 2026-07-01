use serde::{Deserialize, Serialize};

use crate::orchestrator::types::operator_status::{
	execution_program::OperatorExecutionProgramStatus,
	lifecycle::OperatorHistoryLaneStatus,
	post_review::OperatorPostReviewLaneStatus,
	project::{
		OperatorCodexAccountControlStatus, OperatorConnectorBackoffStatus, OperatorProjectStatus,
	},
	queue::OperatorQueuedIssueStatus,
	run::OperatorRunStatus,
	worktree::OperatorWorktreeStatus,
};
use crate::state::CodexAccountActivitySummary;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct OperatorStatusSnapshot {
	pub(crate) project_id: String,
	pub(crate) run_limit: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) status_source: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) snapshot_age_seconds: Option<i64>,
	pub(crate) warnings: Vec<String>,
	pub(crate) warning_details: Vec<OperatorSnapshotWarningDetail>,
	pub(crate) connector_backoffs: Vec<OperatorConnectorBackoffStatus>,
	pub(crate) projects: Vec<OperatorProjectStatus>,
	pub(crate) account_control: OperatorCodexAccountControlStatus,
	pub(crate) accounts: Vec<CodexAccountActivitySummary>,
	pub(crate) current_lanes: Vec<OperatorRunStatus>,
	pub(crate) recent_runs: Vec<OperatorRunStatus>,
	pub(crate) history_lanes: Vec<OperatorHistoryLaneStatus>,
	pub(crate) execution_programs: Vec<OperatorExecutionProgramStatus>,
	pub(crate) queued_candidates: Vec<OperatorQueuedIssueStatus>,
	pub(crate) worktrees: Vec<OperatorWorktreeStatus>,
	pub(crate) post_review_lanes: Vec<OperatorPostReviewLaneStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorSnapshotWarningDetail {
	pub(crate) warning: String,
	pub(crate) project_id: Option<String>,
	pub(crate) repo_root: Option<String>,
	pub(crate) reason: String,
	pub(crate) next_action: Option<String>,
}
