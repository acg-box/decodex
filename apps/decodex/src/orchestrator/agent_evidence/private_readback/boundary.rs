use serde_json::Value;

use crate::orchestrator::{
	PrivateExecutionEvent,
	agent_evidence::{
		AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
		PrivateEvidenceBoundaryCheckSummary, PrivateEvidenceDecisionRequestSummary,
	},
};

pub(super) fn boundary_checks_from_private_events(
	events: &[PrivateExecutionEvent],
) -> Vec<PrivateEvidenceBoundaryCheckSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE)
		.filter_map(boundary_check_from_private_event)
		.collect()
}

pub(super) fn boundary_policy_decision_from_disposition(disposition: &str) -> &'static str {
	match disposition {
		"requires_human" | "insufficient_evidence" => "requires_human_decision",
		_ => "auto_continue",
	}
}

pub(super) fn boundary_policy_requires_enhanced_evidence(policy_decision: &str) -> bool {
	matches!(policy_decision, "requires_enhanced_evidence" | "block_landing")
}

pub(super) fn boundary_policy_blocks_landing(policy_decision: &str) -> bool {
	policy_decision == "block_landing"
}

pub(super) fn authority_decision_requests_from_private_events(
	events: &[PrivateExecutionEvent],
) -> Vec<PrivateEvidenceDecisionRequestSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.filter_map(authority_decision_request_from_private_event)
		.collect()
}

fn boundary_check_from_private_event(
	event: &PrivateExecutionEvent,
) -> Option<PrivateEvidenceBoundaryCheckSummary> {
	let payload = event.payload();
	let disposition = payload.get("disposition")?.as_str()?.to_owned();
	let policy_decision = payload
		.get("policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
		})
		.map(str::to_owned)
		.unwrap_or_else(|| boundary_policy_decision_from_disposition(&disposition).to_owned());
	let reason = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let attempted_recovery_reason =
		payload.get("attempted_recovery_reason").and_then(Value::as_str).map(str::to_owned);
	let decision_contract_count =
		payload.get("decision_contract_ids").and_then(Value::as_array).map_or(0, Vec::len);
	let changed_surface_count =
		payload.get("changed_surfaces").and_then(Value::as_array).map_or(0, Vec::len);
	let improvement_signal_count =
		payload.get("improvement_signals").and_then(Value::as_array).map_or(0, Vec::len);
	let requires_enhanced_evidence = payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| boundary_policy_requires_enhanced_evidence(&policy_decision));
	let blocks_landing = payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| boundary_policy_blocks_landing(&policy_decision));
	let next_action = boundary_check_next_action(&policy_decision);

	Some(PrivateEvidenceBoundaryCheckSummary {
		disposition,
		policy_decision,
		reason,
		attempted_recovery_reason,
		decision_contract_count,
		changed_surface_count,
		improvement_signal_count,
		requires_enhanced_evidence,
		blocks_landing,
		next_action,
	})
}

fn boundary_check_next_action(policy_decision: &str) -> String {
	match policy_decision {
		"auto_continue" =>
			String::from("Continue autonomous architecture recovery inside the accepted boundary."),
		"requires_enhanced_evidence" => String::from(
			"Continue recovery and preserve enhanced evidence before review handoff or landing.",
		),
		"block_landing" => String::from(
			"Continue recovery, but block landing until review or validation policy evidence is restored.",
		),
		"requires_human_decision" =>
			String::from("Stop for a human boundary decision before continuing."),
		_ => String::from("Inspect the authority boundary summary before continuing."),
	}
}

fn authority_decision_request_from_private_event(
	event: &PrivateExecutionEvent,
) -> Option<PrivateEvidenceDecisionRequestSummary> {
	let payload = event.payload();
	let decision_request_id = payload.get("decision_request_id")?.as_str()?.to_owned();
	let reason = payload.get("reason")?.as_str()?.to_owned();
	let boundary = payload.get("boundary")?.as_str()?.to_owned();
	let phase = payload.get("phase").and_then(Value::as_str).unwrap_or("human_required").to_owned();
	let next_action = payload
		.get("next_action")
		.or_else(|| payload.get("resume_condition"))?
		.as_str()?
		.to_owned();
	let recommendation = payload.get("recommendation").and_then(Value::as_str).map(str::to_owned);
	let resume_condition =
		payload.get("resume_condition").and_then(Value::as_str).map(str::to_owned);

	Some(PrivateEvidenceDecisionRequestSummary {
		decision_request_id,
		phase,
		reason,
		boundary,
		next_action,
		recommendation,
		resume_condition,
	})
}
