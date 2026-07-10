use crate::{
	agent::PhaseGoalKind,
	orchestrator::{
		AUTHORITY_DECISION_REQUEST_EVENT_TYPE, IssueRunPlan,
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE,
		RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE, Result, ServiceConfig, StateStore,
		execution_phase_goal::recovery::events::parsing,
	},
	state::{PROGRESS_CHECKPOINT_EVENT_TYPE, PROGRESS_CHECKPOINT_SCHEMA},
};

pub(crate) fn latest_phase_goal_recovery_candidate(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<Option<PhaseGoalKind>> {
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.run_id,
		issue_run.attempt_number,
	)?;
	let mut progress_blockers_cleared = false;

	for event in events.iter().rev() {
		match event.event_type() {
			"phase_goal_completed"
			| "phase_goal_next"
			| "phase_goal_transition"
			| "review_completion_intent"
			| "terminal_finalize" => return Ok(None),
			AUTHORITY_DECISION_REQUEST_EVENT_TYPE => return Ok(None),
			event_type
				if event_type == PROGRESS_CHECKPOINT_EVENT_TYPE
					&& event.matches_contract(
						PROGRESS_CHECKPOINT_EVENT_TYPE,
						PROGRESS_CHECKPOINT_SCHEMA,
						2,
					) && parsing::progress_checkpoint_has_blockers(event.payload())
					&& !progress_blockers_cleared =>
			{
				return Ok(None);
			},
			event_type
				if event_type == PROGRESS_CHECKPOINT_EVENT_TYPE
					&& event.matches_contract(
						PROGRESS_CHECKPOINT_EVENT_TYPE,
						PROGRESS_CHECKPOINT_SCHEMA,
						2,
					) && parsing::progress_checkpoint_clears_blockers(event.payload()) =>
			{
				progress_blockers_cleared = true;
			},
			"phase_goal_set" | "phase_goal_status" => {
				let Some(phase) = parsing::phase_goal_event_phase(event.payload()) else {
					return Ok(None);
				};
				let Some(status) = parsing::phase_goal_event_status(event.payload()) else {
					return Ok(None);
				};

				return Ok(parsing::phase_goal_recovery_candidate_from_status(phase, status));
			},
			_ => {},
		}
	}

	Ok(None)
}

pub(crate) fn latest_open_issue_phase_goal_before_attempt(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	current_run_id: &str,
	current_attempt_number: i64,
) -> Result<Option<PhaseGoalKind>> {
	if current_attempt_number <= 1 {
		return Ok(None);
	}

	let events =
		state_store.list_private_execution_events_for_issue(project.service_id(), issue_id)?;
	let mut progress_blockers_cleared = false;

	for event in events.iter().rev().filter(|event| {
		event.attempt_number() < current_attempt_number && event.run_id() != current_run_id
	}) {
		match event.event_type() {
			"terminal_finalize"
			| "review_completion_intent"
			| AUTHORITY_DECISION_REQUEST_EVENT_TYPE
			| PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE
			| RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE => return Ok(None),
			event_type
				if event_type == PROGRESS_CHECKPOINT_EVENT_TYPE
					&& event.matches_contract(
						PROGRESS_CHECKPOINT_EVENT_TYPE,
						PROGRESS_CHECKPOINT_SCHEMA,
						2,
					) && parsing::progress_checkpoint_has_blockers(event.payload())
					&& !progress_blockers_cleared =>
			{
				return Ok(None);
			},
			event_type
				if event_type == PROGRESS_CHECKPOINT_EVENT_TYPE
					&& event.matches_contract(
						PROGRESS_CHECKPOINT_EVENT_TYPE,
						PROGRESS_CHECKPOINT_SCHEMA,
						2,
					) && parsing::progress_checkpoint_clears_blockers(event.payload()) =>
			{
				progress_blockers_cleared = true;
			},
			PHASE_GOAL_RECOVERY_EVENT_TYPE | "phase_goal_next" | "phase_goal_transition" =>
				if let Some(phase) =
					parsing::phase_goal_continuation_next_phase(event.event_type(), event.payload())
				{
					return Ok(Some(phase));
				},
			"phase_goal_set" | "phase_goal_status" => {
				if let Some(phase) = parsing::phase_goal_active_phase(event.payload()) {
					return Ok(Some(phase));
				}
			},
			_ => {},
		}
	}

	Ok(None)
}
