pub(in crate::recovery::closeout::validation) mod issue;
pub(in crate::recovery::closeout::validation) mod pull_request;
mod worktree;

use crate::{
	prelude::{Result, eyre},
	recovery::{
		closeout::MergedCloseoutValidation, context::RecoveryContext, pull_request_inspection,
		requests::MergedCloseoutRecoveryRequest, review_handoff,
	},
};

pub(in crate::recovery) fn validate_merged_closeout_request(
	context: &RecoveryContext,
	request: &MergedCloseoutRecoveryRequest,
) -> Result<MergedCloseoutValidation> {
	let issue = review_handoff::load_issue_by_identifier(&context.tracker, &request.issue)?;

	issue::validate_merged_closeout_issue_context(context, &issue)?;

	let (landing_state, default_branch) =
		pull_request_inspection::inspect_project_pull_request(context, &request.pr_url)?;

	pull_request::validate_merged_closeout_pull_request(context, &landing_state, &default_branch)?;

	let merge_commit = pull_request_inspection::inspect_project_pull_request_merge_commit(
		context,
		&request.pr_url,
	)?;

	pull_request::ensure_merge_commit_reachable_from_remote_default_branch(
		context.config.repo_root(),
		&request.pr_url,
		&merge_commit,
		&default_branch,
	)?;

	let worktree_mapping = issue::retained_worktree_mapping_for_issue(context, &issue)?;
	let retained_context =
		issue::merged_closeout_retained_context(context, &issue, worktree_mapping.as_ref())?;

	if landing_state.head_ref_name != retained_context.branch_name {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but retained lane branch is `{}`.",
			pull_request_inspection::landing_url(&landing_state),
			landing_state.head_ref_name,
			retained_context.branch_name
		);
	}

	worktree::validate_merged_closeout_worktree_mapping(
		context,
		&issue,
		worktree_mapping.as_ref(),
		&landing_state,
	)?;

	Ok(MergedCloseoutValidation {
		issue,
		branch_name: retained_context.branch_name,
		worktree_path_for_event: retained_context.worktree_path,
		run_id: retained_context.run_id,
		attempt_number: retained_context.attempt_number,
		landing_state,
		merge_commit,
		worktree_mapping,
	})
}
