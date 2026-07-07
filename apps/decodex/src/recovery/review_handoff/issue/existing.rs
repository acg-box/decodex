use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{
		pull_request_inspection, review_handoff_policy,
		review_handoff_policy::{RebindMode, RebindSuccessStateTransition},
	},
	state::{ReviewLifecycleRecord, WorktreeMapping},
	tracker::TrackerIssue,
	workflow::WorkflowTracker,
};

pub(in crate::recovery) fn validate_existing_handoff_refresh(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_lifecycle: &ReviewLifecycleRecord,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	validate_existing_lifecycle_pr_url(issue, worktree, existing_lifecycle, landing_state)?;

	if existing_lifecycle_is_current(worktree, existing_lifecycle, landing_state, local_head_oid) {
		return validate_current_existing_handoff(
			tracker_policy,
			issue,
			worktree,
			existing_lifecycle,
			local_head_oid,
		);
	}

	Ok((
		existing_lifecycle.run_id().to_owned(),
		existing_lifecycle.attempt_number(),
		RebindMode::RefreshExistingHandoff,
	))
}

pub(super) fn validate_rebind_issue_state_for_existing_policy(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<Option<RebindSuccessStateTransition>> {
	review_handoff_policy::validate_rebind_issue_state_for_policy(tracker_policy, issue, mode)
}

fn validate_existing_lifecycle_pr_url(
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_lifecycle: &ReviewLifecycleRecord,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if existing_lifecycle.pr_url() == pull_request_inspection::landing_url(landing_state) {
		return Ok(());
	}

	eyre::bail!(
		"Issue `{}` already has a review lifecycle record for branch `{}` and PR `{}`; refusing to rebind it to `{}`.",
		issue.identifier,
		worktree.branch_name(),
		existing_lifecycle.pr_url(),
		pull_request_inspection::landing_url(landing_state)
	);
}

fn existing_lifecycle_is_current(
	worktree: &WorktreeMapping,
	existing_lifecycle: &ReviewLifecycleRecord,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> bool {
	existing_lifecycle.branch_name() == worktree.branch_name()
		&& existing_lifecycle.pr_url() == pull_request_inspection::landing_url(landing_state)
		&& existing_lifecycle.pr_head_oid() == local_head_oid
		&& existing_lifecycle.head_sha() == local_head_oid
}

fn validate_current_existing_handoff(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_lifecycle: &ReviewLifecycleRecord,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	if issue.state.name == tracker_policy.in_progress_state()
		|| issue.state.name == tracker_policy.failure_state()
	{
		return Ok((
			existing_lifecycle.run_id().to_owned(),
			existing_lifecycle.attempt_number(),
			RebindMode::CompleteExistingHandoffState,
		));
	}

	eyre::bail!(
		"Issue `{}` already has a review lifecycle record for branch `{}` and PR `{}` at head `{local_head_oid}`; no rebind is needed.",
		issue.identifier,
		worktree.branch_name(),
		existing_lifecycle.pr_url()
	);
}
