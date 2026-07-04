use crate::{
	pull_request::PullRequestLandingState,
	recovery::review_handoff_diagnosis::{
		actions,
		binding::{
			diagnostics,
			model::{HandoffBindingDiagnostic, HandoffDiagnosticContext},
		},
	},
};

pub(in crate::recovery) fn pull_request_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	pr_inspection: &PullRequestLandingState,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	if context
		.existing_handoff
		.target_base_ref_name()
		.is_some_and(|base_ref| base_ref != pr_inspection.base_ref_name.as_str())
	{
		return Some(diagnostics::rebind_required_diagnostic(
			"review_handoff_base_mismatch",
			"review_handoff.target_base_ref_name",
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			context.issue_identifier,
			context.existing_handoff.pr_url(),
		));
	}

	let mismatch = if pr_inspection.head_ref_name != context.worktree.branch_name() {
		Some(("pull_request_branch_mismatch", "pull_request.head_ref_name"))
	} else if context.local_head_oid != Some(pr_inspection.head_ref_oid.as_str()) {
		Some(("pull_request_head_mismatch", "pull_request.head_ref_oid"))
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
