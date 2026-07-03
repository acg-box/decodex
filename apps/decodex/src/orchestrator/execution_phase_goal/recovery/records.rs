use color_eyre::Report;

use crate::{
	agent::PhaseGoalKind,
	orchestrator::{
		self, IssueRunPlan, PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE, Result,
		ServiceConfig, StateStore, execution_phase_goal::recovery::events,
	},
};

#[derive(Clone, Copy)]
pub(in crate::orchestrator::execution_phase_goal::recovery) struct PhaseGoalRecoveryRecord<'a> {
	pub(in crate::orchestrator::execution_phase_goal::recovery) project: &'a ServiceConfig,
	pub(in crate::orchestrator::execution_phase_goal::recovery) state_store: &'a StateStore,
	pub(in crate::orchestrator::execution_phase_goal::recovery) issue_run: &'a IssueRunPlan,
	pub(in crate::orchestrator::execution_phase_goal::recovery) source_phase: PhaseGoalKind,
	pub(in crate::orchestrator::execution_phase_goal::recovery) next_phase: PhaseGoalKind,
	pub(in crate::orchestrator::execution_phase_goal::recovery) source_error_class: &'a str,
	pub(in crate::orchestrator::execution_phase_goal::recovery) source_error_message:
		Option<&'a str>,
}

pub(in crate::orchestrator::execution_phase_goal::recovery) fn phase_goal_recovery_source_error_class(
	error: &Report,
) -> &'static str {
	orchestrator::retained_progress_source_error_class(error).unwrap_or("app_server_run_failed")
}

pub(in crate::orchestrator::execution_phase_goal::recovery) fn phase_goal_recovery_source_error_message(
	error: &Report,
) -> String {
	truncate_phase_goal_recovery_error(error.to_string(), 512)
}

pub(in crate::orchestrator::execution_phase_goal::recovery) fn matching_phase_goal_recovery_count(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	source_phase: PhaseGoalKind,
	source_error_class: &str,
) -> Result<i64> {
	let events = state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?;

	Ok(events
		.iter()
		.filter(|event| {
			event.event_type() == PHASE_GOAL_RECOVERY_EVENT_TYPE
				&& events::phase_goal_recovery_event_source_phase(event.payload())
					.is_some_and(|phase| phase == source_phase.as_str())
				&& events::phase_goal_recovery_event_source_error_class(event.payload())
					.is_some_and(|class| class == source_error_class)
		})
		.count() as i64)
}

pub(in crate::orchestrator::execution_phase_goal::recovery) fn record_phase_goal_recovery_continuation(
	record: PhaseGoalRecoveryRecord<'_>,
) -> Result<()> {
	record.state_store.append_private_execution_event(
		record.project.service_id(),
		&record.issue_run.issue.id,
		&record.issue_run.run_id,
		record.issue_run.attempt_number,
		PHASE_GOAL_RECOVERY_EVENT_TYPE,
		serde_json::json!({
			"schema": "decodex.phase_goal_signal/1",
			"phase": record.source_phase.as_str(),
			"signal": "phase_goal_recovered",
			"payload": {
				"nextPhase": record.next_phase.as_str(),
				"sourceErrorClass": record.source_error_class,
				"sourceErrorMessage": record.source_error_message,
			},
		}),
	)?;

	Ok(())
}

pub(in crate::orchestrator::execution_phase_goal::recovery) fn record_phase_goal_recovery_blocked(
	record: PhaseGoalRecoveryRecord<'_>,
	prior_recovery_count: i64,
) -> Result<()> {
	record.state_store.append_private_execution_event(
		record.project.service_id(),
		&record.issue_run.issue.id,
		&record.issue_run.run_id,
		record.issue_run.attempt_number,
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE,
		serde_json::json!({
			"schema": "decodex.phase_goal_signal/1",
			"phase": record.source_phase.as_str(),
			"signal": "continuation_budget_exhausted",
			"payload": {
				"nextPhase": record.next_phase.as_str(),
				"sourceErrorClass": record.source_error_class,
				"sourceErrorMessage": record.source_error_message,
				"priorRecoveryCount": prior_recovery_count,
				"automaticContinuationLimit": PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
			},
		}),
	)?;

	Ok(())
}

fn truncate_phase_goal_recovery_error(value: String, max_chars: usize) -> String {
	if value.chars().count() <= max_chars {
		return value;
	}

	let mut truncated = value.chars().take(max_chars).collect::<String>();

	truncated.push_str("...");

	truncated
}
