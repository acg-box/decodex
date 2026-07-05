use serde_json::Value;

use crate::agent::PhaseGoalKind;

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

pub(in crate::orchestrator::execution_phase_goal::recovery::events) fn progress_checkpoint_has_blockers(
	payload: &Value,
) -> bool {
	payload.get("blockers").is_some_and(|blockers| match blockers {
		Value::Array(items) => !items.is_empty(),
		Value::Null => false,
		_ => true,
	})
}

pub(in crate::orchestrator::execution_phase_goal::recovery::events) fn progress_checkpoint_clears_blockers(
	payload: &Value,
) -> bool {
	payload
		.get("blockers")
		.is_some_and(|blockers| matches!(blockers, Value::Array(items) if items.is_empty()))
}

pub(in crate::orchestrator::execution_phase_goal::recovery::events) fn phase_goal_event_phase(
	payload: &Value,
) -> Option<PhaseGoalKind> {
	payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("phase")?.as_str())
		.and_then(phase_goal_kind_from_str)
}

pub(in crate::orchestrator::execution_phase_goal::recovery::events) fn phase_goal_event_status(
	payload: &Value,
) -> Option<&str> {
	payload
		.get("status")
		.and_then(Value::as_str)
		.or_else(|| payload.get("payload")?.get("status")?.as_str())
}

pub(in crate::orchestrator::execution_phase_goal::recovery::events) fn phase_goal_recovery_candidate_from_status(
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

pub(in crate::orchestrator::execution_phase_goal::recovery::events) fn phase_goal_active_phase(
	payload: &Value,
) -> Option<PhaseGoalKind> {
	let phase = phase_goal_event_phase(payload)?;
	let status = phase_goal_event_status(payload)?;

	(status == "active").then_some(phase)
}

pub(in crate::orchestrator::execution_phase_goal::recovery::events) fn phase_goal_continuation_next_phase(
	event_type: &str,
	payload: &Value,
) -> Option<PhaseGoalKind> {
	let phase = if event_type == "phase_goal_next" {
		payload.get("phase")?.as_str()?
	} else {
		payload.get("payload")?.get("nextPhase")?.as_str()?
	};

	phase_goal_kind_from_str(phase)
}
