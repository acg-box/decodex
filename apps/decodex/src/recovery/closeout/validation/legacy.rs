use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{
		closeout::LegacyCloseoutValidation, context::RecoveryContext, git_worktree,
		pull_request_inspection, requests::LegacyCloseoutRecoveryRequest, review_handoff,
	},
	state::WorktreeMapping,
	tracker::TrackerIssue,
	workflow::WorkflowTracker,
};

pub(in crate::recovery) fn validate_legacy_closeout_request(
	context: &RecoveryContext,
	request: &LegacyCloseoutRecoveryRequest,
) -> Result<LegacyCloseoutValidation> {
	let issue = review_handoff::load_issue_by_identifier(&context.tracker, &request.issue)?;

	validate_legacy_closeout_issue_state(context.workflow.frontmatter().tracker(), &issue)?;

	let worktree = legacy_closeout_worktree(context, &issue)?;

	if !worktree.provenance().is_legacy_unknown() {
		eyre::bail!(
			"Issue `{}` worktree provenance is `{}`; legacy closeout requires `legacy_unknown` cleanup-only provenance.",
			issue.identifier,
			worktree.provenance().source()
		);
	}

	let (landing_state, default_branch) =
		pull_request_inspection::inspect_project_pull_request(context, &request.pr_url)?;

	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{}`.",
			request.pr_url,
			landing_state.base_ref_name,
			default_branch
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; legacy closeout requires `MERGED`.",
			request.pr_url,
			landing_state.state
		);
	}

	let local_head_oid = validate_legacy_closeout_worktree(&worktree, &landing_state)?;
	let merge_commit = pull_request_inspection::inspect_project_pull_request_merge_commit(
		context,
		&request.pr_url,
	)?;
	let worktree_path_for_event = git_worktree::repository_relative_path(
		context.config.repo_root(),
		worktree.worktree_path(),
	);

	Ok(LegacyCloseoutValidation {
		issue,
		worktree,
		landing_state,
		local_head_oid,
		merge_commit,
		worktree_path_for_event,
	})
}

fn validate_legacy_closeout_issue_state(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
) -> Result<()> {
	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(());
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but legacy closeout requires a terminal state: {}.",
		issue.identifier,
		issue.state.name,
		tracker_policy.terminal_states().join(", ")
	)
}

fn legacy_closeout_worktree(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<WorktreeMapping> {
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(worktree);
	}
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.identifier)? {
		return Ok(worktree);
	}

	eyre::bail!("Issue `{}` has no retained worktree mapping.", issue.identifier)
}

fn validate_legacy_closeout_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<String> {
	review_handoff::validate_retained_pr_worktree(worktree, landing_state, "legacy closeout")
}
