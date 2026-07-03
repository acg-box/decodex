use std::time::Instant;

use crate::orchestrator::{
	IssueDispatchMode, IssueTracker, Result, RetryDispatchDecision, RetryKind, RetryQueue,
	ServiceConfig, StateStore, TargetIssueRunContext, WorkflowDocument, run_target_issue_once,
};
use crate::orchestrator::daemon_retry::{
	clear_retry_schedule_and_release, retry_entry_is_temporarily_blocked,
};

pub(crate) fn plan_due_retry_run<T>(
	retry_queue: &mut RetryQueue,
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<RetryDispatchDecision>
where
	T: IssueTracker,
{
	let now = Instant::now();
	let Some(first_entry) = retry_queue.next_entry().cloned() else {
		return Ok(RetryDispatchDecision::Continue);
	};

	if now < first_entry.ready_at {
		tracing::debug!(
			issue_id = first_entry.issue_id,
			retry_kind = ?first_entry.kind,
			retry_attempt = first_entry.attempt,
			"Retry queue is holding the project claim until the next retry is due."
		);

		return Ok(RetryDispatchDecision::Blocked {
			excluded_issue_ids: queued_retry_issue_ids(retry_queue),
		});
	}

	let mut blocked_issue_id = None;

	for entry in retry_queue.ordered_entries() {
		if now < entry.ready_at {
			break;
		}

		let preferred_issue_state = (entry.kind == RetryKind::Continuation
			&& !matches!(
				entry.dispatch_mode,
				IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout
			))
		.then_some(workflow.frontmatter().tracker().in_progress_state());
		let Some(summary) = run_target_issue_once(TargetIssueRunContext {
			tracker,
			project,
			workflow,
			state_store,
			issue_id: &entry.issue_id,
			preferred_issue_state,
			preferred_initial_issue_state: entry.continuation_initial_issue_state.as_deref(),
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: entry.dispatch_mode,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})?
		else {
			if retry_entry_is_temporarily_blocked(tracker, project, workflow, state_store, &entry)?
			{
				blocked_issue_id.get_or_insert_with(|| entry.issue_id.clone());

				continue;
			}

			clear_retry_schedule_and_release(retry_queue, state_store, &entry.issue_id)?;

			continue;
		};

		return Ok(RetryDispatchDecision::Dispatch(Box::new(summary)));
	}

	Ok(blocked_issue_id.map_or(RetryDispatchDecision::Continue, |_issue_id| {
		RetryDispatchDecision::Blocked { excluded_issue_ids: queued_retry_issue_ids(retry_queue) }
	}))
}

pub(super) fn queued_retry_issue_ids(retry_queue: &RetryQueue) -> Vec<String> {
	retry_queue.ordered_entries().into_iter().map(|entry| entry.issue_id).collect()
}
