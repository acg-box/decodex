use serde::{Deserialize, Serialize};

use crate::orchestrator::types::operator_status::loop_status::OperatorLoopStatus;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorQueuedIssueStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) title: String,
	pub(crate) author: Option<String>,
	pub(crate) state: String,
	pub(crate) priority: Option<i64>,
	pub(crate) created_at: String,
	pub(crate) classification: String,
	pub(crate) reason: String,
	pub(crate) attention: Option<OperatorQueuedIssueAttentionStatus>,
	pub(crate) blocker_identifiers: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorQueuedIssueAttentionStatus {
	pub(crate) summary: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) decision_request: Option<OperatorAuthorityDecisionRequestStatus>,
	pub(crate) run_id: Option<String>,
	pub(crate) attempt_number: Option<i64>,
	pub(crate) current_operation: Option<String>,
	pub(crate) thread_status: Option<String>,
	pub(crate) attempt_status: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) loop_status: Option<OperatorLoopStatus>,
	pub(crate) auto_retry_blocked_reason: Option<String>,
	pub(crate) attention_error_class: Option<String>,
	pub(crate) attention_next_action: Option<String>,
	pub(crate) retry_budget_attempt_count: Option<i64>,
	pub(crate) retry_budget_max_attempts: i64,
	pub(crate) last_activity_at: Option<String>,
	pub(crate) last_progress_at: Option<String>,
	pub(crate) last_event_type: Option<String>,
	pub(crate) event_count: i64,
	pub(crate) process_alive: Option<bool>,
	pub(crate) process_liveness_reason: Option<String>,
	pub(crate) worktree_path: Option<String>,
	pub(crate) worktree_has_tracked_changes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorAuthorityDecisionRequestStatus {
	pub(crate) phase: String,
	pub(crate) reason: String,
	pub(crate) boundary: String,
	pub(crate) decision_request_id: String,
	pub(crate) next_action: String,
	pub(crate) recommendation: Option<String>,
	pub(crate) resume_condition: Option<String>,
}
