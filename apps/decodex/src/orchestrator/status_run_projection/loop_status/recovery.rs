use crate::orchestrator::{
	ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, OperatorArchitectureRecoveryStatus,
	OperatorRecoveryBudgetStatus, PrivateExecutionEvent, Value, status_run_projection,
};

pub(crate) fn operator_architecture_recovery_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorArchitectureRecoveryStatus> {
	if !matches!(
		event.event_type(),
		ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
			| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
			| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
	) {
		return None;
	}

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
				.map(status_run_projection::operator_boundary_policy_decision_from_disposition)
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
			boundary_policy_decision.as_deref().is_some_and(
				status_run_projection::operator_boundary_policy_requires_enhanced_evidence,
			)
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
			boundary_policy_decision
				.as_deref()
				.is_some_and(status_run_projection::operator_boundary_policy_blocks_landing)
		});
	let recovery_budget_attempt = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("attempt"))
		.and_then(Value::as_u64);
	let recovery_budget_max_attempts = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("max_attempts"))
		.and_then(Value::as_u64);
	let budget = recovery_budget_attempt
		.zip(recovery_budget_max_attempts)
		.map(|(attempt, max_attempts)| OperatorRecoveryBudgetStatus { attempt, max_attempts });
	let next_action = operator_architecture_recovery_next_action(
		&reason_code,
		boundary_policy_decision.as_deref(),
		requires_enhanced_evidence,
		blocks_landing,
	);

	Some(OperatorArchitectureRecoveryStatus {
		status: operator_architecture_recovery_status_for_reason(&reason_code).to_owned(),
		reason_code,
		guardrail_reason,
		boundary_disposition,
		boundary_policy_decision,
		requires_enhanced_evidence,
		blocks_landing,
		round: recovery_budget_attempt,
		budget,
		next_action,
	})
}

pub(crate) fn operator_architecture_recovery_status_for_reason(reason_code: &str) -> &'static str {
	match reason_code {
		"architecture_recovery_started" => "active",
		"architecture_recovery_exhausted" => "exhausted",
		"contract_boundary_required" | "external_dependency_required" => "human_required",
		_ => "terminal",
	}
}

pub(crate) fn operator_architecture_recovery_next_action(
	reason_code: &str,
	policy_decision: Option<&str>,
	requires_enhanced_evidence: bool,
	blocks_landing: bool,
) -> String {
	match reason_code {
		"architecture_recovery_started" => {
			match (policy_decision, blocks_landing, requires_enhanced_evidence) {
				(Some(policy), true, _) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`; keep landing blocked until validation or review-policy evidence is restored."
				),
				(Some(policy), false, true) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`; preserve enhanced evidence before review handoff or landing."
				),
				(Some(policy), false, false) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`."
				),
				(None, true, _) => String::from(
					"Retry with a materially different implementation strategy; keep landing blocked until validation or review-policy evidence is restored.",
				),
				(None, false, true) => String::from(
					"Retry with a materially different implementation strategy; preserve enhanced evidence before review handoff or landing.",
				),
				(None, false, false) => String::from(
					"Retry with a materially different implementation strategy inside authority.",
				),
			}
		},
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
