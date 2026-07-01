use serde_json::Value;

use crate::orchestrator::{
	OperatorPhaseAcceptanceStatus, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PrivateExecutionEvent,
	ProtocolActivitySummary, RUN_OPERATION_REVIEW_WRITEBACK, TERMINAL_GUARDED_RUN_STATUS,
};

pub(super) fn operator_run_active_goal_phase(events: &[PrivateExecutionEvent]) -> Option<String> {
	for event in events.iter().rev() {
		if matches!(event.event_type(), "phase_goal_completed" | "phase_goal_transition") {
			return None;
		}
		if !matches!(event.event_type(), "phase_goal_set" | "phase_goal_status") {
			continue;
		}

		let payload = event.payload();
		let nested = payload.get("payload").unwrap_or(payload);
		let status = nested.get("status").or_else(|| payload.get("status")).and_then(Value::as_str);

		if status.is_some_and(|value| matches!(value, "complete" | "completed" | "blocked")) {
			return None;
		}

		return nested
			.get("phase")
			.or_else(|| payload.get("phase"))
			.and_then(Value::as_str)
			.map(str::to_owned);
	}

	None
}

pub(super) fn operator_run_public_progress_phase(
	events: &[PrivateExecutionEvent],
) -> Option<String> {
	events.iter().rev().find_map(|event| {
		(event.event_type() == "progress_checkpoint")
			.then_some(event.payload())
			.and_then(|payload| payload.get("phase"))
			.and_then(Value::as_str)
			.map(str::to_owned)
	})
}

pub(super) fn operator_run_phase_acceptance_status(
	events: &[PrivateExecutionEvent],
) -> Option<OperatorPhaseAcceptanceStatus> {
	let event = events
		.iter()
		.rev()
		.find(|event| event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE)?;
	let payload = event.payload();
	let phase = payload.get("phase")?.as_str()?.to_owned();
	let decision = payload.get("decision")?.as_str()?.to_owned();
	let reason_code = payload.get("reason_code")?.as_str()?.to_owned();
	let objective_covered = payload
		.get("objective_coverage")
		.and_then(|objective| objective.get("covered"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let effective_delta_present = payload
		.get("effective_delta")
		.and_then(|delta| delta.get("present"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let changed_surfaces = payload
		.get("effective_delta")
		.and_then(|delta| delta.get("changed_surfaces"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let non_goal_passed = payload
		.get("non_goal_check")
		.and_then(|check| check.get("passed"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let validation_passed = payload
		.get("validation_evidence")
		.and_then(|evidence| evidence.get("repo_gate_passed"))
		.and_then(Value::as_bool)
		.unwrap_or(false);

	Some(OperatorPhaseAcceptanceStatus {
		phase,
		decision,
		reason_code,
		objective_covered,
		effective_delta_present,
		changed_surfaces,
		non_goal_passed,
		validation_passed,
		recorded_at: event.recorded_at().to_owned(),
		run_id: event.run_id().to_owned(),
		attempt_number: event.attempt_number(),
		next_action: payload
			.get("next_action")
			.and_then(Value::as_str)
			.unwrap_or("inspect_phase_acceptance_check")
			.to_owned(),
	})
}

pub(super) fn operator_run_wait_reason(
	phase: &str,
	wait_reason: Option<String>,
	protocol_activity: Option<&ProtocolActivitySummary>,
) -> Option<String> {
	if wait_reason.is_some() || phase != "executing" {
		return wait_reason;
	}

	protocol_activity
		.and_then(|summary| summary.waiting_reason.clone())
		.filter(|reason| reason != "turn_completed")
}

pub(super) fn operator_run_default_review_phase(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<&'static str> {
	if operator_run_has_terminal_lifecycle(status, phase, current_operation) {
		return None;
	}
	if current_operation == RUN_OPERATION_REVIEW_WRITEBACK {
		return Some("handoff");
	}

	None
}

pub(super) fn operator_run_lifecycle_loop_summary(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<String> {
	operator_run_has_terminal_lifecycle(status, phase, current_operation)
		.then(|| format!("terminal lifecycle: {status}"))
}

pub(super) fn operator_run_has_terminal_lifecycle(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> bool {
	phase == "completed"
		|| phase == "terminal_pending"
		|| current_operation == "ledger_outcome"
		|| matches!(
			status,
			"succeeded"
				| "failed" | "interrupted"
				| "review_handoff_pending"
				| "review_repair_pending"
				| "closeout_pending"
				| "manual_attention_pending"
				| "cleanup_complete"
				| "closeout" | "landed"
				| "manual_attention"
				| TERMINAL_GUARDED_RUN_STATUS
		)
}
