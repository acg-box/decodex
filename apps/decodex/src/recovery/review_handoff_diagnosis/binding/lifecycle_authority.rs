use crate::{
	recovery::{
		git_worktree::{self, ReviewHandoffLineage},
		review_handoff_diagnosis::binding::{
			diagnostics,
			model::{HandoffBindingDiagnostic, HandoffDiagnosticContext},
		},
	},
	state::ReviewLifecycleRecord,
};

pub(in crate::recovery) fn lifecycle_authority_head_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	local_head_oid: &str,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	let mismatch = if context.existing_lifecycle.pr_head_oid() != local_head_oid {
		match git_worktree::worktree_head_descends_from_review_handoff(
			context.worktree.worktree_path(),
			context.existing_lifecycle.pr_head_oid(),
			local_head_oid,
		) {
			ReviewHandoffLineage::Descends => None,
			ReviewHandoffLineage::Diverged =>
				Some(("review_lifecycle_lineage_mismatch", "review_lifecycle.pr_head_oid")),
			ReviewHandoffLineage::Unknown =>
				Some(("review_lifecycle_lineage_check_failed", "review_lifecycle.pr_head_oid")),
		}
	} else {
		lifecycle_authority_binding_mismatch(context, context.existing_lifecycle, local_head_oid)
	};

	mismatch.map(|(reason, field)| {
		diagnostics::rebind_required_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			context.issue_identifier,
			context.existing_lifecycle.pr_url(),
		)
	})
}

fn lifecycle_authority_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	lifecycle: &ReviewLifecycleRecord,
	local_head_oid: &str,
) -> Option<(&'static str, &'static str)> {
	if lifecycle.branch_name() != context.worktree.branch_name() {
		Some(("review_lifecycle_branch_mismatch", "review_lifecycle.branch_name"))
	} else if lifecycle.head_sha() != local_head_oid {
		Some(("review_lifecycle_head_mismatch", "review_lifecycle.head_sha"))
	} else {
		None
	}
}
