use crate::{orchestrator::OperatorWorktreeProvenanceStatus, state::WorktreeMapping};

pub(crate) fn operator_worktree_provenance_from_mapping(
	mapping: &WorktreeMapping,
) -> OperatorWorktreeProvenanceStatus {
	operator_worktree_provenance(
		mapping.provenance().source(),
		mapping.provenance().created_at_unix(),
		mapping.provenance().updated_at_unix(),
	)
}

pub(crate) fn operator_worktree_provenance(
	source: &str,
	created_at_unix: Option<i64>,
	updated_at_unix: Option<i64>,
) -> OperatorWorktreeProvenanceStatus {
	OperatorWorktreeProvenanceStatus {
		source: source.to_owned(),
		created_at_unix,
		updated_at_unix,
		audit_required: false,
	}
}
