use crate::recovery::review_handoff_diagnosis::{
	actions,
	binding::{
		diagnostics,
		model::{HandoffBindingDiagnostic, HandoffDiagnosticContext},
	},
};

pub(in crate::recovery) fn worktree_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	let mismatch = if context.existing_handoff.branch_name() != context.worktree.branch_name() {
		Some(("review_handoff_branch_mismatch", "review_handoff.branch_name"))
	} else if context.local_branch_name.is_none() {
		Some(("worktree_checkout_branch_missing", "worktree.local_branch"))
	} else if context.local_branch_name != Some(context.worktree.branch_name()) {
		Some(("worktree_checkout_branch_mismatch", "worktree.local_branch"))
	} else if context.worktree_clean == Some(false) {
		Some(("worktree_dirty", "worktree.clean"))
	} else if context.local_head_oid.is_none() {
		Some(("worktree_head_missing", "worktree.local_head"))
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		diagnostics::mismatched_handoff_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			actions::inspect_handoff_next_action(
				context.issue_identifier,
				context.existing_handoff.pr_url(),
			),
		)
	})
}
