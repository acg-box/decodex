use std::collections::HashMap;

use crate::{
	orchestrator::{
		OperatorExecutionProgramStatus, OperatorHistoryLedgerOutcome, OperatorReviewRouteCount,
	},
	tracker::records::LinearExecutionEventRecord,
};

pub(in crate::orchestrator) struct OperatorHistoryLedgerRecord {
	pub(in crate::orchestrator) record: LinearExecutionEventRecord,
	pub(in crate::orchestrator) event_unix_epoch: Option<i64>,
	pub(in crate::orchestrator) sort_unix_epoch: Option<i64>,
	pub(in crate::orchestrator) comment_index: usize,
}

pub(in crate::orchestrator) struct OperatorIssueDisplayMetadata {
	pub(in crate::orchestrator) issue_identifier: String,
	pub(in crate::orchestrator) title: Option<String>,
	pub(in crate::orchestrator) author: Option<String>,
	pub(in crate::orchestrator) issue_state: Option<String>,
	pub(in crate::orchestrator) active_label_present: Option<bool>,
	pub(in crate::orchestrator) needs_attention_label_present: Option<bool>,
}

pub(in crate::orchestrator) struct WorktreeOwnership {
	pub(in crate::orchestrator) kind: &'static str,
	pub(in crate::orchestrator) reason: String,
	pub(in crate::orchestrator) next_action: Option<String>,
	pub(in crate::orchestrator) audit_required: bool,
}

pub(in crate::orchestrator) struct OperatorLifecycleMetricPhase {
	pub(in crate::orchestrator) key: &'static str,
	pub(in crate::orchestrator) label: &'static str,
	pub(in crate::orchestrator) rank: u8,
}
#[derive(Default)]
pub(in crate::orchestrator) struct OperatorLaneTerminalProjection {
	pub(in crate::orchestrator) outcomes_by_issue_key:
		HashMap<String, OperatorHistoryLedgerOutcome>,
}

pub(in crate::orchestrator) struct OperatorExecutionProgramReadback {
	pub(in crate::orchestrator) statuses: Vec<OperatorExecutionProgramStatus>,
	pub(in crate::orchestrator) issue_metadata_unavailable: bool,
}

pub(in crate::orchestrator) struct OperatorReviewCheckpointSummaryFields {
	pub(in crate::orchestrator) review_class: Option<String>,
	pub(in crate::orchestrator) risk_class: Option<String>,
	pub(in crate::orchestrator) compact_eligible: Option<bool>,
	pub(in crate::orchestrator) fallback_reason: Option<String>,
	pub(in crate::orchestrator) active_fingerprints: Vec<String>,
	pub(in crate::orchestrator) stop_fingerprint: Option<String>,
	pub(in crate::orchestrator) route_counts: Vec<OperatorReviewRouteCount>,
	pub(in crate::orchestrator) route_next_action: Option<String>,
}
