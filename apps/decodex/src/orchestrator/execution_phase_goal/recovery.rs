use color_eyre::Report;
use serde_json::{Value, json};

use super::{
	super::{
		AUTHORITY_DECISION_REQUEST_EVENT_TYPE, IssueRunPlan,
		PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT, PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE,
		PHASE_GOAL_RECOVERY_EVENT_TYPE, RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE, Result,
		RunSummary, ServiceConfig, StateStore, WorkflowDocument,
		retained_progress_source_error_class, run_summary_from_issue_run,
		worktree_has_tracked_changes,
	},
	RepoGatePhaseGoalController,
};
use crate::agent::{PhaseGoalKind, PhaseGoalTransition};

pub(in crate::orchestrator) struct PhaseGoalRecoveryContinuation {
	pub(in crate::orchestrator) source_phase: PhaseGoalKind,
	pub(in crate::orchestrator) next_phase: PhaseGoalKind,
}

#[derive(Clone, Copy)]
struct PhaseGoalRecoveryRecord<'a> {
	project: &'a ServiceConfig,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
	source_phase: PhaseGoalKind,
	next_phase: PhaseGoalKind,
	source_error_class: &'a str,
	source_error_message: Option<&'a str>,
}

pub(in crate::orchestrator) fn maybe_continue_after_phase_goal_recovery(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<RunSummary>> {
	let source_error_message = phase_goal_recovery_source_error_message(error);
	let Some(recovery) = recover_phase_goal_continuation(
		project,
		workflow,
		state_store,
		issue_run,
		phase_goal_recovery_source_error_class(error),
		Some(source_error_message.as_str()),
	)?
	else {
		return Ok(None);
	};
	let mut summary = run_summary_from_issue_run(project.service_id(), issue_run);

	summary.continuation_pending = true;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		source_phase = recovery.source_phase.as_str(),
		next_phase = recovery.next_phase.as_str(),
		error = %error,
		"Recovered phase goal after app-server failure; scheduling continuation."
	);

	Ok(Some(summary))
}

pub(in crate::orchestrator) fn recover_phase_goal_continuation(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	source_error_class: &str,
	source_error_message: Option<&str>,
) -> Result<Option<PhaseGoalRecoveryContinuation>> {
	if !worktree_has_tracked_changes(&issue_run.worktree.path) {
		return Ok(None);
	}

	let Some(source_phase) = latest_phase_goal_recovery_candidate(project, state_store, issue_run)?
	else {
		return Ok(None);
	};
	let controller = RepoGatePhaseGoalController { project, workflow, state_store, issue_run };
	let transition = controller.validate_phase_goal_output(source_phase)?;
	let next_phase = match transition {
		PhaseGoalTransition::Continue(next_goal) => next_goal.phase,
		PhaseGoalTransition::CompleteRun => return Ok(None),
	};
	let prior_recovery_count = matching_phase_goal_recovery_count(
		project,
		state_store,
		issue_run,
		source_phase,
		source_error_class,
	)?;
	let recovery_record = PhaseGoalRecoveryRecord {
		project,
		state_store,
		issue_run,
		source_phase,
		next_phase,
		source_error_class,
		source_error_message,
	};

	if prior_recovery_count >= PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT {
		record_phase_goal_recovery_blocked(recovery_record, prior_recovery_count)?;

		return Ok(None);
	}

	record_phase_goal_recovery_continuation(recovery_record)?;

	Ok(Some(PhaseGoalRecoveryContinuation { source_phase, next_phase }))
}

fn phase_goal_recovery_source_error_class(error: &Report) -> &'static str {
	retained_progress_source_error_class(error).unwrap_or("app_server_run_failed")
}

fn phase_goal_recovery_source_error_message(error: &Report) -> String {
	truncate_phase_goal_recovery_error(error.to_string(), 512)
}

fn truncate_phase_goal_recovery_error(value: String, max_chars: usize) -> String {
	if value.chars().count() <= max_chars {
		return value;
	}

	let mut truncated = value.chars().take(max_chars).collect::<String>();

	truncated.push_str("...");

	truncated
}

pub(in crate::orchestrator) fn latest_phase_goal_recovery_candidate(
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
			"progress_checkpoint"
				if progress_checkpoint_has_blockers(event.payload())
					&& !progress_blockers_cleared =>
			{
				return Ok(None);
			},
			"progress_checkpoint" if progress_checkpoint_clears_blockers(event.payload()) => {
				progress_blockers_cleared = true;
			},
			"phase_goal_set" | "phase_goal_status" => {
				let Some(phase) = phase_goal_event_phase(event.payload()) else {
					return Ok(None);
				};
				let Some(status) = phase_goal_event_status(event.payload()) else {
					return Ok(None);
				};

				return Ok(phase_goal_recovery_candidate_from_status(phase, status));
			},
			_ => {},
		}
	}

	Ok(None)
}

pub(in crate::orchestrator) fn latest_open_issue_phase_goal_before_attempt(
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
			"progress_checkpoint"
				if progress_checkpoint_has_blockers(event.payload())
					&& !progress_blockers_cleared =>
			{
				return Ok(None);
			},
			"progress_checkpoint" if progress_checkpoint_clears_blockers(event.payload()) => {
				progress_blockers_cleared = true;
			},
			PHASE_GOAL_RECOVERY_EVENT_TYPE | "phase_goal_next" | "phase_goal_transition" =>
				if let Some(phase) =
					phase_goal_continuation_next_phase(event.event_type(), event.payload())
				{
					return Ok(Some(phase));
				},
			"phase_goal_set" | "phase_goal_status" => {
				if let Some(phase) = phase_goal_active_phase(event.payload()) {
					return Ok(Some(phase));
				}
			},
			_ => {},
		}
	}

	Ok(None)
}

pub(in crate::orchestrator) fn issue_has_blocking_lane_decision_evidence(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
) -> Result<bool> {
	let events =
		state_store.list_private_execution_events_for_issue(project.service_id(), issue_id)?;

	for event in events.iter().rev() {
		match event.event_type() {
			"terminal_finalize" | "review_completion_intent" => return Ok(false),
			AUTHORITY_DECISION_REQUEST_EVENT_TYPE | PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE => {
				return Ok(true);
			},
			"lane_decision"
				if event.payload().get("next_action").and_then(Value::as_str).is_some_and(
					|action| {
						matches!(
							action,
							"needs_attention" | "stop_blocked" | "forbidden_stale_or_ambiguous"
						)
					},
				) =>
			{
				return Ok(true);
			},
			"progress_checkpoint" if progress_checkpoint_has_blockers(event.payload()) => {
				return Ok(true);
			},
			"progress_checkpoint" if progress_checkpoint_clears_blockers(event.payload()) => {
				return Ok(false);
			},
			"phase_goal_next" | "phase_goal_transition" | "phase_goal_completed" => {
				return Ok(false);
			},
			_ => {},
		}
	}

	Ok(false)
}

fn progress_checkpoint_has_blockers(payload: &Value) -> bool {
	payload.get("blockers").is_some_and(|blockers| match blockers {
		Value::Array(items) => !items.is_empty(),
		Value::Null => false,
		_ => true,
	})
}

fn progress_checkpoint_clears_blockers(payload: &Value) -> bool {
	payload
		.get("blockers")
		.is_some_and(|blockers| matches!(blockers, Value::Array(items) if items.is_empty()))
}

fn phase_goal_event_phase(payload: &Value) -> Option<PhaseGoalKind> {
	payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("phase")?.as_str())
		.and_then(phase_goal_kind_from_str)
}

fn phase_goal_event_status(payload: &Value) -> Option<&str> {
	payload
		.get("status")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("status")?.as_str())
}

fn phase_goal_recovery_candidate_from_status(
	phase: PhaseGoalKind,
	status: &str,
) -> Option<PhaseGoalKind> {
	if status != "active" {
		return None;
	}
	if matches!(
		phase,
		PhaseGoalKind::ImplementToValidationReady
			| PhaseGoalKind::RepairValidationFailures
			| PhaseGoalKind::RepairAcceptedReviewFindings
	) {
		Some(phase)
	} else {
		None
	}
}

fn phase_goal_active_phase(payload: &Value) -> Option<PhaseGoalKind> {
	let phase = phase_goal_event_phase(payload)?;
	let status = phase_goal_event_status(payload)?;

	(status == "active").then_some(phase)
}

fn matching_phase_goal_recovery_count(
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
				&& phase_goal_recovery_event_source_phase(event.payload())
					.is_some_and(|phase| phase == source_phase.as_str())
				&& phase_goal_recovery_event_source_error_class(event.payload())
					.is_some_and(|class| class == source_error_class)
		})
		.count() as i64)
}

fn phase_goal_recovery_event_source_phase(payload: &Value) -> Option<&str> {
	payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("sourcePhase")?.as_str())
}

fn phase_goal_recovery_event_source_error_class(payload: &Value) -> Option<&str> {
	payload.get("payload")?.get("sourceErrorClass")?.as_str()
}

fn phase_goal_continuation_next_phase(event_type: &str, payload: &Value) -> Option<PhaseGoalKind> {
	let phase = if event_type == "phase_goal_next" {
		payload.get("phase")?.as_str()?
	} else {
		payload.get("payload")?.get("nextPhase")?.as_str()?
	};

	phase_goal_kind_from_str(phase)
}

fn record_phase_goal_recovery_continuation(record: PhaseGoalRecoveryRecord<'_>) -> Result<()> {
	record.state_store.append_private_execution_event(
		record.project.service_id(),
		&record.issue_run.issue.id,
		&record.issue_run.run_id,
		record.issue_run.attempt_number,
		PHASE_GOAL_RECOVERY_EVENT_TYPE,
		json!({
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

fn record_phase_goal_recovery_blocked(
	record: PhaseGoalRecoveryRecord<'_>,
	prior_recovery_count: i64,
) -> Result<()> {
	record.state_store.append_private_execution_event(
		record.project.service_id(),
		&record.issue_run.issue.id,
		&record.issue_run.run_id,
		record.issue_run.attempt_number,
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE,
		json!({
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

pub(super) fn phase_goal_kind_from_str(value: &str) -> Option<PhaseGoalKind> {
	match value {
		"implement_to_validation_ready" => Some(PhaseGoalKind::ImplementToValidationReady),
		"repair_validation_failures" => Some(PhaseGoalKind::RepairValidationFailures),
		"repair_accepted_review_findings" => Some(PhaseGoalKind::RepairAcceptedReviewFindings),
		"review_repair_evidence" => Some(PhaseGoalKind::ReviewRepairEvidence),
		"handoff_evidence" => Some(PhaseGoalKind::HandoffEvidence),
		_ => None,
	}
}
