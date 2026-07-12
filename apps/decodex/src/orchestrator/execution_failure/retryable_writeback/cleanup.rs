use crate::orchestrator::execution_failure::{
	self, FailureHandlingContext, IssueDispatchMode, IssueTracker,
	RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE, Report, Result,
	retained_progress_source_error_class, retryable_writeback::validation,
};

pub(in crate::orchestrator::execution_failure::retryable_writeback) fn cleanup_retryable_failed_start_ownership<
	T,
>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	if !retryable_failed_start_cleanup_allowed(context, error)? {
		return Ok(());
	}

	let tracker_policy = context.workflow.frontmatter().tracker();
	let failure_state_name = tracker_policy.failure_state();
	let failure_state_is_startable =
		tracker_policy.startable_states().iter().any(|state| state == failure_state_name);

	if !failure_state_is_startable {
		tracing::warn!(
			issue_id = context.issue_run.issue.id,
			issue = context.issue_run.issue.identifier,
			target_state = failure_state_name,
			"Retryable failed-start cleanup skipped because the configured failure state is not startable."
		);

		return Ok(());
	}

	let Some(state_id) = context.issue_run.issue.state_id_for_name(failure_state_name) else {
		tracing::warn!(
			issue_id = context.issue_run.issue.id,
			issue = context.issue_run.issue.identifier,
			target_state = failure_state_name,
			"Retryable failed-start cleanup skipped because the target state id was not available."
		);

		return Ok(());
	};

	context.tracker.update_issue_state(&context.issue_run.issue.id, state_id)?;

	execution_failure::ensure_automation_activity_label(
		context.tracker,
		&context.issue_run.issue,
		context.project.service_id(),
		false,
	)?;

	context.state_store.clear_worktree(&context.issue_run.issue.id)?;
	context
		.state_store
		.append_private_execution_event(
			context.project.service_id(),
			&context.issue_run.issue.id,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
			RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE,
			execution_failure::json!({
				"schema": "decodex.retryable_failed_start_cleanup/1",
				"source_error_class": retained_progress_source_error_class(error)
					.unwrap_or("retryable_execution_failure"),
				"dispatch_mode": context.issue_run.dispatch_mode.as_str(),
				"active_label_cleared": true,
				"worktree_mapping_cleared": true,
				"target_issue_state": failure_state_name,
				"issue_state_reset": true,
				"retryable_by_next_program_pass": true,
			}),
		)
		.map(|_| ())?;

	tracing::info!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		issue_state_reset = true,
		"Cleared retryable failed-start ownership after a no-diff Program run failure."
	);

	Ok(())
}

fn retryable_failed_start_cleanup_allowed<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
) -> Result<bool>
where
	T: IssueTracker,
{
	if context.issue_run.dispatch_mode != IssueDispatchMode::Program {
		return Ok(false);
	}
	if !validation::failure_precedes_effective_agent_execution(error) {
		return Ok(false);
	}
	if context
		.state_store
		.claim_for_lane(context.project.service_id(), &context.issue_run.issue.id)?
		.is_some()
	{
		return Ok(false);
	}
	if context.state_store.issue_has_review_lifecycle_record(
		context.project.service_id(),
		&context.issue_run.issue.id,
	)? {
		return Ok(false);
	}
	if execution_failure::latest_open_issue_phase_goal_before_attempt(
		context.project,
		context.state_store,
		&context.issue_run.issue.id,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
	)?
	.is_some()
	{
		return Ok(false);
	}

	Ok(execution_failure::loop_guardrail_worktree_fingerprint(&context.issue_run.worktree.path)?
		.is_some_and(|fingerprint| !fingerprint.effective_delta_present))
}
