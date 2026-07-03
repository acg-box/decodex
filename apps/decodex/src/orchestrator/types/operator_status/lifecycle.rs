use serde::{Deserialize, Serialize};

use crate::{
	orchestrator::types::operator_status::run::OperatorRunStatus, state::ChildAgentActivityBucket,
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorHistoryLaneStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) title: Option<String>,
	pub(crate) author: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) issue_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) active_label_present: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) needs_attention_label_present: Option<bool>,
	pub(crate) issue_key: String,
	pub(crate) attempt_count: usize,
	pub(crate) ledger_outcome: OperatorHistoryLedgerOutcome,
	pub(crate) lifecycle_metrics: OperatorLaneLifecycleMetrics,
	pub(crate) latest_run: OperatorRunStatus,
	pub(crate) attempts: Vec<OperatorRunStatus>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorLaneLifecycleMetrics {
	pub(crate) attempt_count: usize,
	pub(crate) run_count: usize,
	pub(crate) recorded_attempt_count: usize,
	pub(crate) recovered_attempt_count: usize,
	pub(crate) current_snapshot_attempt_count: usize,
	pub(crate) captured_attempt_count: usize,
	pub(crate) missing_attempt_count: usize,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) wall_seconds: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens_current: Option<i64>,
	pub(crate) input_tokens_peak: Option<i64>,
	pub(crate) input_tokens_cumulative: i64,
	pub(crate) output_tokens_cumulative: i64,
	pub(crate) largest_tool_output_bytes: Option<i64>,
	pub(crate) largest_tool_output_tool: Option<String>,
	pub(crate) large_output_warnings: Vec<String>,
	pub(crate) buckets: Vec<ChildAgentActivityBucket>,
	pub(crate) phases: Vec<OperatorLaneLifecyclePhaseMetrics>,
	pub(crate) attempt_evidence: Vec<OperatorLaneLifecycleAttemptEvidence>,
	pub(crate) recovery_gaps: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorLaneLifecyclePhaseMetrics {
	pub(crate) phase: String,
	pub(crate) label: String,
	pub(crate) attempt_count: usize,
	pub(crate) run_count: usize,
	pub(crate) recorded_attempt_count: usize,
	pub(crate) recovered_attempt_count: usize,
	pub(crate) current_snapshot_attempt_count: usize,
	pub(crate) captured_attempt_count: usize,
	pub(crate) missing_attempt_count: usize,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) wall_seconds: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens_current: Option<i64>,
	pub(crate) input_tokens_peak: Option<i64>,
	pub(crate) input_tokens_cumulative: i64,
	pub(crate) output_tokens_cumulative: i64,
	pub(crate) largest_tool_output_bytes: Option<i64>,
	pub(crate) largest_tool_output_tool: Option<String>,
	pub(crate) large_output_warnings: Vec<String>,
	pub(crate) buckets: Vec<ChildAgentActivityBucket>,
	pub(crate) attempt_evidence: Vec<OperatorLaneLifecycleAttemptEvidence>,
	pub(crate) recovery_gaps: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorLaneLifecycleAttemptEvidence {
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) status: String,
	pub(crate) phase: String,
	pub(crate) source: String,
	pub(crate) evidence: Vec<String>,
	pub(crate) gaps: Vec<String>,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorHistoryLedgerOutcome {
	pub(crate) ledger_status: String,
	pub(crate) final_outcome: String,
	pub(crate) final_event_type: Option<String>,
	pub(crate) final_event_at: Option<String>,
	pub(crate) summary: Option<String>,
	pub(crate) pr_url: Option<String>,
	pub(crate) commit_sha: Option<String>,
	pub(crate) branch: Option<String>,
	pub(crate) closeout_status: Option<String>,
	pub(crate) needs_attention_reason: Option<String>,
	pub(crate) lifecycle_started_at: Option<String>,
	pub(crate) lifecycle_finished_at: Option<String>,
	pub(crate) lifecycle_elapsed_seconds: Option<i64>,
	pub(crate) record_count: usize,
}
