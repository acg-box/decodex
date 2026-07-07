use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{
		context::RecoveryContext,
		review_handoff::issue::{existing, missing},
		review_handoff_policy::{RebindMode, RebindSuccessStateTransition},
	},
	state::{ReviewLifecycleRecord, WorktreeMapping},
	tracker::TrackerIssue,
};

pub(in crate::recovery) fn validate_rebind_existing_handoff(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_lifecycle: Option<&ReviewLifecycleRecord>,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	let Some(existing_lifecycle) = existing_lifecycle else {
		let attempt =
			context.state_store.latest_run_attempt_for_issue(&issue.id)?.ok_or_else(|| {
				eyre::eyre!("Issue `{}` has no recorded run attempt to rebind.", issue.identifier)
			})?;

		return Ok((
			attempt.run_id().to_owned(),
			attempt.attempt_number(),
			missing::missing_handoff_rebind_mode(
				context,
				issue,
				worktree,
				landing_state,
				local_head_oid,
				&attempt,
			)?,
		));
	};

	existing::validate_existing_handoff_refresh(
		context.workflow.frontmatter().tracker(),
		issue,
		worktree,
		existing_lifecycle,
		landing_state,
		local_head_oid,
	)
}

pub(in crate::recovery) fn validate_rebind_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<WorktreeMapping> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;

	Ok(worktree)
}

pub(in crate::recovery) fn validate_rebind_issue_state(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<Option<RebindSuccessStateTransition>> {
	existing::validate_rebind_issue_state_for_existing_policy(
		context.workflow.frontmatter().tracker(),
		issue,
		mode,
	)
}
