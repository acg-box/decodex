use crate::orchestrator::dispatch_policy::{
	self, CloseoutDispatchEligibility, IssueTracker, PullRequestReviewStateInspector, Result,
	RetainedCloseoutPrMergeGate, ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
	WorktreeManager, WorktreeSpec,
};

pub(crate) fn evaluate_closeout_dispatch_policy_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<CloseoutDispatchEligibility>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	if !closeout_issue_state_is_eligible(tracker, issue, project, workflow)? {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}

	let Some(worktree) = closeout_worktree(issue, project, state_store)? else {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	};
	let Some(lifecycle_record) = state_store.review_lifecycle_record(
		project.service_id(),
		&issue.id,
		&worktree.branch_name,
	)?
	else {
		return Ok(CloseoutDispatchEligibility::Blocked("missing_review_handoff_record"));
	};

	if lifecycle_record.branch_name() != worktree.branch_name {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}

	Ok(closeout_merge_gate_eligibility(
		dispatch_policy::retained_closeout_pr_merge_gate_with_inspector(
			&worktree.path,
			&worktree.branch_name,
			lifecycle_record.pr_url(),
			review_state_inspector,
		)?,
	))
}

pub(crate) fn issue_passes_closeout_dispatch_policy_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	Ok(matches!(
		evaluate_closeout_dispatch_policy_with_inspector(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)?,
		CloseoutDispatchEligibility::Eligible
	))
}

pub(crate) fn closeout_dispatch_block_reason_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<Option<&'static str>>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	Ok(
		match evaluate_closeout_dispatch_policy_with_inspector(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)? {
			CloseoutDispatchEligibility::Blocked(reason) => Some(reason),
			CloseoutDispatchEligibility::Eligible | CloseoutDispatchEligibility::Ineligible => None,
		},
	)
}

fn closeout_issue_state_is_eligible<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();
	let issue_state = issue.state.name.as_str();

	Ok(!issue.has_label(tracker_policy.opt_out_label())
		&& !issue.has_label(tracker_policy.needs_attention_label())
		&& dispatch_policy::issue_has_service_ownership(tracker, issue, project.service_id())?
		&& (issue_state == tracker_policy.success_state() || issue_state == completed_state))
}

fn closeout_worktree(
	issue: &TrackerIssue,
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<Option<WorktreeSpec>> {
	Ok(match state_store.worktree_for_issue(&issue.id)? {
		Some(mapping) => {
			if mapping.project_id() != project.service_id()
				|| !mapping.worktree_path().try_exists()?
			{
				return Ok(None);
			}

			Some(WorktreeSpec {
				branch_name: mapping.branch_name().to_owned(),
				issue_identifier: issue.identifier.clone(),
				path: mapping.worktree_path().to_path_buf(),
				reused_existing: true,
			})
		},
		None => planned_closeout_worktree(issue, project)?,
	})
}

fn planned_closeout_worktree(
	issue: &TrackerIssue,
	project: &ServiceConfig,
) -> Result<Option<WorktreeSpec>> {
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let planned_worktree = worktree_manager.plan_for_issue(&issue.identifier);

	if planned_worktree.path.try_exists()? { Ok(Some(planned_worktree)) } else { Ok(None) }
}

fn closeout_merge_gate_eligibility(
	merge_gate: RetainedCloseoutPrMergeGate,
) -> CloseoutDispatchEligibility {
	match merge_gate {
		RetainedCloseoutPrMergeGate::Merged => CloseoutDispatchEligibility::Eligible,
		RetainedCloseoutPrMergeGate::NotMerged => {
			CloseoutDispatchEligibility::Blocked("pull_request_not_merged")
		},
		RetainedCloseoutPrMergeGate::PullRequestStateReadFailed => {
			CloseoutDispatchEligibility::Blocked("pull_request_state_read_failed")
		},
	}
}
