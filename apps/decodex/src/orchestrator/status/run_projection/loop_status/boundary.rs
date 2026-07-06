use crate::orchestrator::{
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, OperatorBoundaryStatus, PrivateExecutionEvent, Value,
};

pub(crate) fn operator_boundary_policy_decision_from_disposition(
	disposition: &str,
) -> &'static str {
	match disposition {
		"requires_human" | "insufficient_evidence" => "requires_human_decision",
		_ => "auto_continue",
	}
}

pub(crate) fn operator_boundary_policy_requires_enhanced_evidence(policy_decision: &str) -> bool {
	matches!(policy_decision, "requires_enhanced_evidence" | "block_landing")
}

pub(crate) fn operator_boundary_policy_blocks_landing(policy_decision: &str) -> bool {
	policy_decision == "block_landing"
}

pub(crate) fn operator_boundary_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorBoundaryStatus> {
	if event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE {
		return None;
	}

	let payload = event.payload();
	let disposition = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("disposition"))
		.and_then(Value::as_str)
		.or_else(|| payload.get("disposition").and_then(Value::as_str))?
		.to_owned();
	let reason = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let policy_decision = payload
		.get("policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
		})
		.map(str::to_owned)
		.unwrap_or_else(|| {
			operator_boundary_policy_decision_from_disposition(&disposition).to_owned()
		});
	let attempted_recovery_reason =
		payload.get("attempted_recovery_reason").and_then(Value::as_str).map(str::to_owned);
	let changed_surface_count =
		payload.get("changed_surfaces").and_then(Value::as_array).map_or(0, Vec::len);
	let improvement_signal_count =
		payload.get("improvement_signals").and_then(Value::as_array).map_or(0, Vec::len);
	let requires_enhanced_evidence = payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| operator_boundary_policy_requires_enhanced_evidence(&policy_decision));
	let blocks_landing = payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| operator_boundary_policy_blocks_landing(&policy_decision));

	Some(OperatorBoundaryStatus {
		disposition,
		policy_decision,
		reason,
		attempted_recovery_reason,
		changed_surface_count,
		improvement_signal_count,
		requires_enhanced_evidence,
		blocks_landing,
	})
}
