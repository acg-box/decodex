//! Report DTOs for explicit operator recovery commands.

use serde::Serialize;

use super::{
	GHOST_LANE_CLASSIFICATION, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
	STALE_ACTIVE_CLASSIFICATION, STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION,
};

#[derive(Serialize)]
pub(super) struct ReviewHandoffRecoveryReport {
	pub(super) project_id: String,
	pub(super) diagnostics: Vec<ReviewHandoffDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct ReviewHandoffDiagnostic {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) issue_identifier: String,
	pub(super) issue_state: String,
	pub(super) classification: String,
	pub(super) reason: String,
	pub(super) branch_name: String,
	pub(super) worktree_path: String,
	pub(super) local_branch_name: Option<String>,
	pub(super) local_head_oid: Option<String>,
	pub(super) worktree_clean: Option<bool>,
	pub(super) existing_pr_url: Option<String>,
	pub(super) existing_lifecycle_handoff_head_oid: Option<String>,
	pub(super) existing_lifecycle_phase_head_oid: Option<String>,
	pub(super) pr_base_ref: Option<String>,
	pub(super) pr_head_oid: Option<String>,
	pub(super) mismatched_field: Option<String>,
	pub(super) active_label_present: Option<bool>,
	pub(super) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct GhostLaneRecoveryReport {
	pub(super) project_id: String,
	pub(super) diagnostics: Vec<GhostLaneDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct GhostLaneDiagnostic {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) issue_identifier: Option<String>,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) attempt_status: String,
	pub(super) classification: String,
	pub(super) reason: String,
	pub(super) run_lease: bool,
	pub(super) control_channel: String,
	pub(super) evidence: Vec<String>,
	pub(super) blockers: Vec<String>,
	pub(super) next_action: String,
}
impl GhostLaneDiagnostic {
	pub(super) fn recoverable(&self) -> bool {
		(self.classification == GHOST_LANE_CLASSIFICATION
			|| self.classification == MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION)
			&& self.blockers.is_empty()
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct StaleActiveRecoveryReport {
	pub(super) project_id: String,
	pub(super) diagnostics: Vec<StaleActiveDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct StaleActiveDiagnostic {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) issue_identifier: String,
	pub(super) issue_state: String,
	pub(super) classification: String,
	pub(super) reason: String,
	pub(super) queue_label_present: bool,
	pub(super) active_label_present: bool,
	pub(super) needs_attention_label_present: bool,
	pub(super) latest_run_id: Option<String>,
	pub(super) latest_attempt_number: Option<i64>,
	pub(super) latest_attempt_status: Option<String>,
	pub(super) run_lease: bool,
	pub(super) active_shared_claim: bool,
	pub(super) control_channel: String,
	pub(super) worktree_path: Option<String>,
	pub(super) worktree_state: String,
	pub(super) evidence: Vec<String>,
	pub(super) blockers: Vec<String>,
	pub(super) next_action: String,
}
impl StaleActiveDiagnostic {
	pub(super) fn recoverable(&self) -> bool {
		matches!(
			self.classification.as_str(),
			STALE_ACTIVE_CLASSIFICATION | STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION
		) && self.blockers.is_empty()
	}
}
