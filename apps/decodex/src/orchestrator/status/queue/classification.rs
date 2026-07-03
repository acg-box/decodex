use crate::{
	orchestrator::status::{
		self, IssueTracker, LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailReason,
		ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON, QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT,
		ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
		queue::{guardrail, models::QueuedGuardrailCommand},
	},
	prelude::Result,
	tracker,
};

pub(super) fn classify_queued_issue_with_command<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<(&'static str, &'static str, Option<QueuedGuardrailCommand>)>
where
	T: IssueTracker,
{
	let tracker_policy = workflow.frontmatter().tracker();

	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(("closed", "terminal_state", None));
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(("blocked", "issue_needs_attention", None));
	}
	if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
		return Ok(("claimed", "shared_claim_present", None));
	}
	if (issue.state.name == tracker_policy.in_progress_state()
		|| tracker_policy.startable_states().iter().any(|state| state == &issue.state.name))
		&& status::ordinary_dispatch_blocked_by_retained_review_handoff(
			project.service_id(),
			issue,
			state_store,
		)? {
		return Ok(("blocked", ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON, None));
	}
	if tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(project.service_id()),
	)? {
		return Ok(("blocked", QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT, None));
	}
	if !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name) {
		return Ok(("blocked", "non_startable_state", None));
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(("blocked", "issue_opted_out", None));
	}
	if !status::todo_blocker_rule_passes(issue, workflow) {
		let checkpoint_count = guardrail::current_dependency_program_stale_count(
			project,
			workflow,
			state_store,
			issue,
		)?;
		let reason = if checkpoint_count >= LOOP_GUARDRAIL_CONVERGENCE_BUDGET {
			LoopGuardrailReason::DependencyProgramStale.error_class()
		} else {
			"open_tracker_blockers"
		};

		return Ok((
			"blocked",
			reason,
			Some(guardrail::observe_dependency_program_stale_guardrail_command(issue)),
		));
	}

	let clear_guardrail_command =
		guardrail::dependency_program_stale_checkpoint_exists(project, state_store, issue)?
			.then(|| guardrail::clear_dependency_program_stale_guardrail_command(issue));

	if !status::issue_has_generic_dispatch_briefing(issue) {
		return Ok(("blocked", "missing_dispatch_briefing", clear_guardrail_command));
	}

	let queue_label = tracker::automation_queue_label(project.service_id());

	if !status::issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)? {
		return Ok(("blocked", "dispatch_policy_rejected", clear_guardrail_command));
	}

	Ok(("ready", "eligible_for_dispatch", clear_guardrail_command))
}
