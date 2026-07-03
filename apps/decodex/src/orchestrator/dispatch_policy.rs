use crate::{
	config::ServiceConfig,
	orchestrator::{IssueDispatchMode, RetryIssueStateHint},
	prelude::Result,
	state::StateStore,
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

mod closeout;
mod description;
pub(in crate::orchestrator) mod lifecycle;
mod retry_budget;

pub(in crate::orchestrator) use closeout::{
	closeout_dispatch_block_reason, evaluate_closeout_dispatch_policy_with_inspector,
	issue_passes_closeout_dispatch_policy, issue_passes_review_repair_dispatch_policy,
};
#[cfg(test)]
pub(crate) use closeout::{
	closeout_dispatch_block_reason_with_inspector,
	issue_passes_closeout_dispatch_policy_with_inspector,
};
pub(in crate::orchestrator::dispatch_policy) use description::description_is_machine_only_fenced_block;
pub(in crate::orchestrator) use description::render_issue_description_for_prompt;
pub(in crate::orchestrator) use lifecycle::{
	cleanup_completed_post_review_lane, cleanup_terminal_worktree, cleanup_worktree_mapping,
	clear_recovered_issue_lease, clear_worktree_retry_schedule, is_issue_eligible,
	is_issue_in_progress_for_run, is_issue_not_dispatchable_for_current_dispatch,
	is_terminal_issue, mark_run_attempt_if_active, refresh_issue, state_name_is_terminal,
	todo_blocker_rule_passes,
};
pub(in crate::orchestrator) use retry_budget::{
	clear_terminal_guard_marker, issue_has_service_ownership, issue_passes_retry_dispatch_policy,
	issue_passes_retry_retention_policy, issue_retry_budget_exhausted,
	issue_retry_budget_exhausted_for_worktree, retry_budget_base_for_dispatch_mode,
	retry_budget_base_for_issue_worktree, write_retry_budget_marker, write_terminal_guard_marker,
};

pub(in crate::orchestrator) const ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON: &str =
	"review_handoff_state_transition_pending";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) enum CloseoutDispatchEligibility {
	Eligible,
	Ineligible,
	Blocked(&'static str),
}

pub(crate) fn issue_has_generic_dispatch_briefing(issue: &TrackerIssue) -> bool {
	!description_is_machine_only_fenced_block(&issue.description)
}

pub(in crate::orchestrator) fn ordinary_dispatch_blocked_by_retained_review_handoff(
	project_id: &str,
	issue: &TrackerIssue,
	state_store: &StateStore,
) -> Result<bool> {
	let Some(worktree) = state_store.worktree_for_issue(&issue.id)? else {
		return Ok(false);
	};

	if worktree.project_id() != project_id || !worktree.worktree_path().try_exists()? {
		return Ok(false);
	}

	let Some(review_handoff) =
		state_store.review_handoff_marker(project_id, &issue.id, worktree.branch_name())?
	else {
		return Ok(false);
	};

	Ok(review_handoff.branch_name() == worktree.branch_name())
}

pub(in crate::orchestrator) fn issue_passes_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	queue_label: &str,
	queue_membership_confirmed_by_source: bool,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();

	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(false);
	}
	if !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name) {
		return Ok(false);
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(false);
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(false);
	}
	if !queue_membership_confirmed_by_source {
		if issue.labels_complete {
			if !issue.has_label(queue_label) {
				return Ok(false);
			}
		} else if !tracker::issue_has_label_with_server_confirmation(tracker, issue, queue_label)? {
			return Ok(false);
		}
	}
	if !todo_blocker_rule_passes(issue, workflow) {
		return Ok(false);
	}
	if !issue_has_generic_dispatch_briefing(issue) {
		return Ok(false);
	}

	Ok(true)
}

pub(in crate::orchestrator) fn issue_passes_current_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dispatch_mode: IssueDispatchMode,
	hint: RetryIssueStateHint<'_>,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	match dispatch_mode {
		IssueDispatchMode::Normal => {
			let queue_label = tracker::automation_queue_label(project.service_id());

			Ok(issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, false)?
				&& !ordinary_dispatch_blocked_by_retained_review_handoff(
					project.service_id(),
					issue,
					state_store,
				)?)
		},
		IssueDispatchMode::Program => {
			let queue_label = tracker::automation_queue_label(project.service_id());

			Ok(issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)?
				&& !ordinary_dispatch_blocked_by_retained_review_handoff(
					project.service_id(),
					issue,
					state_store,
				)?)
		},
		IssueDispatchMode::Retry => {
			issue_passes_retry_dispatch_policy(tracker, issue, project, workflow, state_store, hint)
		},
		IssueDispatchMode::ReviewRepair => {
			Ok(issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)?
				&& !issue_retry_budget_exhausted(workflow, state_store, &issue.id)?)
		},
		IssueDispatchMode::Closeout => {
			issue_passes_closeout_dispatch_policy(tracker, issue, project, workflow, state_store)
		},
	}
}
