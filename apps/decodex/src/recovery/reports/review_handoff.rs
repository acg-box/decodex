use serde::Serialize;

#[derive(Serialize)]
pub(in crate::recovery) struct ReviewHandoffRecoveryReport {
	pub(in crate::recovery) project_id: String,
	pub(in crate::recovery) diagnostics: Vec<ReviewHandoffDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::recovery) struct ReviewHandoffDiagnostic {
	pub(in crate::recovery) project_id: String,
	pub(in crate::recovery) issue_id: String,
	pub(in crate::recovery) issue_identifier: String,
	pub(in crate::recovery) issue_state: String,
	pub(in crate::recovery) classification: String,
	pub(in crate::recovery) reason: String,
	pub(in crate::recovery) branch_name: String,
	pub(in crate::recovery) worktree_path: String,
	pub(in crate::recovery) local_branch_name: Option<String>,
	pub(in crate::recovery) local_head_oid: Option<String>,
	pub(in crate::recovery) worktree_clean: Option<bool>,
	pub(in crate::recovery) existing_pr_url: Option<String>,
	pub(in crate::recovery) existing_lifecycle_handoff_head_oid: Option<String>,
	pub(in crate::recovery) existing_lifecycle_phase_head_oid: Option<String>,
	pub(in crate::recovery) pr_base_ref: Option<String>,
	pub(in crate::recovery) pr_head_oid: Option<String>,
	pub(in crate::recovery) pr_read_error: Option<String>,
	pub(in crate::recovery) mismatched_field: Option<String>,
	pub(in crate::recovery) active_label_present: Option<bool>,
	pub(in crate::recovery) next_action: String,
}
