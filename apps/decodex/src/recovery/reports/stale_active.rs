use serde::Serialize;

use crate::recovery::{STALE_ACTIVE_CLASSIFICATION, STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::recovery) struct StaleActiveRecoveryReport {
	pub(in crate::recovery) project_id: String,
	pub(in crate::recovery) diagnostics: Vec<StaleActiveDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::recovery) struct StaleActiveDiagnostic {
	pub(in crate::recovery) project_id: String,
	pub(in crate::recovery) issue_id: String,
	pub(in crate::recovery) issue_identifier: String,
	pub(in crate::recovery) issue_state: String,
	pub(in crate::recovery) classification: String,
	pub(in crate::recovery) reason: String,
	pub(in crate::recovery) queue_label_present: bool,
	pub(in crate::recovery) active_label_present: bool,
	pub(in crate::recovery) needs_attention_label_present: bool,
	pub(in crate::recovery) latest_run_id: Option<String>,
	pub(in crate::recovery) latest_attempt_number: Option<i64>,
	pub(in crate::recovery) latest_attempt_status: Option<String>,
	pub(in crate::recovery) run_lease: bool,
	pub(in crate::recovery) active_shared_claim: bool,
	pub(in crate::recovery) control_channel: String,
	pub(in crate::recovery) worktree_path: Option<String>,
	pub(in crate::recovery) worktree_state: String,
	pub(in crate::recovery) evidence: Vec<String>,
	pub(in crate::recovery) blockers: Vec<String>,
	pub(in crate::recovery) next_action: String,
}
impl StaleActiveDiagnostic {
	pub(in crate::recovery) fn recoverable(&self) -> bool {
		matches!(
			self.classification.as_str(),
			STALE_ACTIVE_CLASSIFICATION | STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION
		) && self.blockers.is_empty()
	}
}
