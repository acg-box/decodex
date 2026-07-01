use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorWorktreeStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) issue_state: Option<String>,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: String,
	pub(crate) ownership: String,
	pub(crate) ownership_reason: String,
	pub(crate) provenance: OperatorWorktreeProvenanceStatus,
	pub(crate) recovery_next_action: Option<String>,
	pub(crate) hygiene: Option<OperatorWorktreeHygieneStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorWorktreeProvenanceStatus {
	pub(crate) source: String,
	pub(crate) created_at_unix: Option<i64>,
	pub(crate) updated_at_unix: Option<i64>,
	pub(crate) audit_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorWorktreeHygieneStatus {
	pub(crate) classification: String,
	pub(crate) default_branch: String,
	pub(crate) dirty: bool,
	pub(crate) reason: String,
}
