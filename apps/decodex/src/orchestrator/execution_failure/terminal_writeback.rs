mod apply;
mod prepare;
mod record;

use crate::orchestrator::{
	execution_failure::{
		self, HarnessOutcomeKind, IssueRunPlan, IssueTracker, Report, Result,
		TerminalFailureOutcome, TerminalFailureWritebackRuntime, WorkflowDocument,
	},
	records::LinearExecutionEventPublicProjection,
};

#[derive(Clone, Copy, Eq, PartialEq)]
enum TerminalFailureEventRecordStatus {
	Recorded,
	Duplicate,
	NoLocalStore,
}

struct PreparedTerminalFailureWriteback {
	failure_state_id: String,
	needs_attention_label: String,
	needs_attention_label_id: Option<String>,
	terminal_failure_state_name: String,
	projection: LinearExecutionEventPublicProjection,
	error_class: &'static str,
	retry_guarded_by_state: bool,
}

pub(crate) fn apply_terminal_failure_writeback<T>(
	tracker: &T,
	runtime: TerminalFailureWritebackRuntime<'_>,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
	manual_attention_requested: bool,
	error: &Report,
) -> Result<TerminalFailureOutcome>
where
	T: IssueTracker,
{
	let writeback = prepare::prepare_terminal_failure_writeback(
		tracker,
		runtime,
		workflow,
		issue_run,
		worktree_path,
		manual_attention_requested,
		error,
	)?;
	let event_status =
		record::record_terminal_failure_writeback_event(tracker, runtime, issue_run, &writeback)?;

	if event_status == TerminalFailureEventRecordStatus::Duplicate {
		return Ok(terminal_failure_outcome(&writeback));
	}

	let writeback_result =
		apply::apply_terminal_failure_tracker_writeback(tracker, runtime, issue_run, &writeback);

	if let Err(error) = writeback_result {
		record::forget_terminal_failure_writeback_event(runtime, event_status, &writeback)?;

		return Err(error);
	}
	if let Some(state_store) = runtime.state_store {
		let outcome = if writeback.projection.record.event_type == "needs_attention" {
			HarnessOutcomeKind::ManualAttention
		} else {
			HarnessOutcomeKind::TerminalFailure
		};

		execution_failure::record_harness_outcome_best_effort(
			state_store,
			runtime.service_id,
			issue_run,
			outcome,
			Some(writeback.error_class),
			None,
			writeback.projection.record.pr_url.as_deref(),
		);
	}

	Ok(terminal_failure_outcome(&writeback))
}

fn terminal_failure_outcome(
	writeback: &PreparedTerminalFailureWriteback,
) -> TerminalFailureOutcome {
	TerminalFailureOutcome {
		error_class: writeback.error_class,
		retry_guarded_by_state: writeback.retry_guarded_by_state,
	}
}
