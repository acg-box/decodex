use crate::orchestrator::{
	RunAttempt, TrackerIssue,
	daemon_retry::{
		self, ChildExitRetryContext, IssueDispatchMode, IssueTracker, LaneDecisionSnapshot, Result,
		RetryKind,
	},
};

pub(crate) fn child_exit_lane_decision_permits_retry<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	run_attempt: &RunAttempt,
	dispatch_mode: IssueDispatchMode,
	kind: RetryKind,
) -> Result<bool>
where
	T: IssueTracker,
{
	let lane_snapshot = LaneDecisionSnapshot::child_exit_retry(
		issue.identifier.clone(),
		run_attempt.run_id().to_owned(),
		run_attempt.attempt_number(),
		dispatch_mode,
		kind == RetryKind::Continuation,
		Some(kind),
		0,
		false,
		false,
	);
	let lane_decision = daemon_retry::decide_lane_next_action(&lane_snapshot);

	context.state_store.append_private_execution_event(
		context.project.service_id(),
		issue.id.as_str(),
		run_attempt.run_id(),
		run_attempt.attempt_number(),
		"lane_decision",
		lane_snapshot.to_json(lane_decision.next_action, lane_decision.reason),
	)?;

	let retry_permitted = !lane_decision.blocks_automatic_execution()
		&& lane_decision.permits_child_exit_retry_kind(kind);

	if !retry_permitted {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue.id.as_str(),
		)?;
	}

	Ok(retry_permitted)
}
