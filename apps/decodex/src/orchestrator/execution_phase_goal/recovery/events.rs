use serde_json::Value;

use crate::{
	agent::PhaseGoalKind,
	orchestrator::{
		AUTHORITY_DECISION_REQUEST_EVENT_TYPE, IssueRunPlan,
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE,
		RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE, Result, ServiceConfig, StateStore,
	},
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

pub(crate) fn issue_has_blocking_lane_decision_evidence(
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
			"lane_decision" if lane_decision_event_blocks_automatic_recovery(event.payload()) => {
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

pub(crate) fn phase_goal_kind_from_str(value: &str) -> Option<PhaseGoalKind> {
	match value {
		"implement_to_validation_ready" => Some(PhaseGoalKind::ImplementToValidationReady),
		"repair_validation_failures" => Some(PhaseGoalKind::RepairValidationFailures),
		"repair_accepted_review_findings" => Some(PhaseGoalKind::RepairAcceptedReviewFindings),
		"review_repair_evidence" => Some(PhaseGoalKind::ReviewRepairEvidence),
		"handoff_evidence" => Some(PhaseGoalKind::HandoffEvidence),
		_ => None,
	}
}

pub(in crate::orchestrator::execution_phase_goal::recovery) fn phase_goal_recovery_event_source_phase(
	payload: &Value,
) -> Option<&str> {
	payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("sourcePhase")?.as_str())
}

pub(in crate::orchestrator::execution_phase_goal::recovery) fn phase_goal_recovery_event_source_error_class(
	payload: &Value,
) -> Option<&str> {
	payload.get("payload")?.get("sourceErrorClass")?.as_str()
}

fn lane_decision_event_blocks_automatic_recovery(payload: &Value) -> bool {
	if let Some(kernel_decision) = payload.get("kernel_decision") {
		return kernel_decision
			.get("decision_class")
			.and_then(Value::as_str)
			.is_some_and(|decision_class| decision_class == "manual_intervention_required")
			|| kernel_decision.get("command_intents").and_then(Value::as_array).is_some_and(
				|intents| {
					intents.iter().any(|intent| {
						intent.get("kind").and_then(Value::as_str)
							== Some("request_manual_intervention")
					})
				},
			);
	}

	payload.get("next_action").and_then(Value::as_str).is_some_and(|action| {
		matches!(action, "needs_attention" | "stop_blocked" | "forbidden_stale_or_ambiguous")
	})
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

fn phase_goal_continuation_next_phase(event_type: &str, payload: &Value) -> Option<PhaseGoalKind> {
	let phase = if event_type == "phase_goal_next" {
		payload.get("phase")?.as_str()?
	} else {
		payload.get("payload")?.get("nextPhase")?.as_str()?
	};

	phase_goal_kind_from_str(phase)
}
