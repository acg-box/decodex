use serde_json::Value;

use crate::orchestrator::{
	PrivateExecutionEvent, VALIDATION_EVIDENCE_SCHEMA,
	agent_evidence::{PrivateEvidenceValidationSummary, VALIDATION_EVIDENCE_EVENT_TYPE},
};

pub(super) fn validation_evidence_from_private_events(
	events: &[PrivateExecutionEvent],
) -> Vec<PrivateEvidenceValidationSummary> {
	events
		.iter()
		.filter(|event| {
			event.matches_contract(VALIDATION_EVIDENCE_EVENT_TYPE, VALIDATION_EVIDENCE_SCHEMA, 2)
		})
		.filter_map(validation_evidence_from_private_event)
		.collect()
}

fn validation_evidence_from_private_event(
	event: &PrivateExecutionEvent,
) -> Option<PrivateEvidenceValidationSummary> {
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

	Some(PrivateEvidenceValidationSummary {
		phase,
		decision,
		reason_code,
		objective_covered,
		effective_delta_present,
		changed_surfaces,
		non_goal_passed,
		validation_passed,
		next_action: payload
			.get("next_action")
			.and_then(Value::as_str)
			.unwrap_or("inspect_validation_evidence")
			.to_owned(),
	})
}
