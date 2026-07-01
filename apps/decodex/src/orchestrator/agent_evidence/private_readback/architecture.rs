use serde_json::Value;

use super::{
	super::{
		ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
		ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, PrivateEvidenceArchitectureRecoverySummary,
		state,
	},
	boundary::{
		boundary_policy_blocks_landing, boundary_policy_decision_from_disposition,
		boundary_policy_requires_enhanced_evidence,
	},
};

pub(super) fn architecture_recoveries_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceArchitectureRecoverySummary> {
	events
		.iter()
		.filter(|event| {
			matches!(
				event.event_type(),
				ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
					| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
					| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
			)
		})
		.filter_map(architecture_recovery_from_private_event)
		.collect()
}

fn architecture_recovery_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceArchitectureRecoverySummary> {
	let payload = event.payload();
	let reason_code = payload.get("reason_code")?.as_str()?.to_owned();
	let guardrail_reason = payload
		.get("guardrail_reason")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("loop_guardrail")
				.and_then(|guardrail| guardrail.get("reason"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let boundary_disposition = payload
		.get("boundary_disposition")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("disposition"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let boundary_policy_decision = payload
		.get("boundary_policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("policy_decision"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned)
		.or_else(|| {
			boundary_disposition
				.as_deref()
				.map(boundary_policy_decision_from_disposition)
				.map(str::to_owned)
		});
	let requires_enhanced_evidence = payload
		.get("requires_enhanced_evidence")
		.and_then(Value::as_bool)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("requires_enhanced_evidence"))
				.and_then(Value::as_bool)
		})
		.unwrap_or_else(|| {
			boundary_policy_decision
				.as_deref()
				.is_some_and(boundary_policy_requires_enhanced_evidence)
		});
	let blocks_landing = payload
		.get("blocks_landing")
		.and_then(Value::as_bool)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("blocks_landing"))
				.and_then(Value::as_bool)
		})
		.unwrap_or_else(|| {
			boundary_policy_decision.as_deref().is_some_and(boundary_policy_blocks_landing)
		});
	let recovery_budget_attempt = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("attempt"))
		.and_then(Value::as_u64);
	let recovery_budget_max_attempts = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("max_attempts"))
		.and_then(Value::as_u64);
	let next_action = architecture_recovery_next_action(&reason_code);

	Some(PrivateEvidenceArchitectureRecoverySummary {
		reason_code,
		guardrail_reason,
		boundary_disposition,
		boundary_policy_decision,
		requires_enhanced_evidence,
		blocks_landing,
		recovery_budget_attempt,
		recovery_budget_max_attempts,
		next_action,
	})
}

fn architecture_recovery_next_action(reason_code: &str) -> String {
	match reason_code {
		"architecture_recovery_started" => String::from(
			"Retry with a materially different implementation strategy inside authority.",
		),
		"architecture_recovery_exhausted" => String::from(
			"Require a new accepted recovery strategy or architecture decision before retrying.",
		),
		"external_dependency_required" => String::from(
			"Resolve the dependency or Execution Program readiness blocker before retrying.",
		),
		"contract_boundary_required" => String::from(
			"Resolve the Decision Contract or Authority Envelope boundary before retrying.",
		),
		_ => String::from("Inspect the Architecture Recovery Packet before retrying."),
	}
}
