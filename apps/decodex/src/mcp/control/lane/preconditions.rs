use serde_json::Value;

use crate::mcp::{self, control::lane::args::LaneControlToolArgs};

pub(super) fn lane_control_preconditions(params: &LaneControlToolArgs) -> Value {
	let authority = params.authority.as_ref();

	serde_json::json!({
		"project_id_present": mcp::non_empty_string(params.project_id.as_deref()).is_some(),
		"issue_present": mcp::non_empty_string(params.issue.as_deref()).is_some(),
		"run_id_present": mcp::non_empty_string(params.run_id.as_deref()).is_some(),
		"expected_turn_id_present": mcp::non_empty_string(params.expected_turn_id.as_deref()).is_some(),
		"message_present": mcp::non_empty_string(params.message.as_deref()).is_some(),
		"force_requested": params.force.unwrap_or(false),
		"authority_reason_present": authority
			.and_then(|value| mcp::non_empty_string(value.reason.as_deref()))
			.is_some(),
		"authority_source_present": authority
			.and_then(|value| mcp::non_empty_string(value.source.as_deref()))
			.is_some(),
		"authority_inspected_run_id_present": authority
			.and_then(|value| mcp::non_empty_string(value.inspected_run_id.as_deref()))
			.is_some(),
		"authority_expected_turn_id_present": authority
			.and_then(|value| mcp::non_empty_string(value.expected_turn_id.as_deref()))
			.is_some(),
		"authority_allow_hard_fallback": authority
			.and_then(|value| value.allow_hard_fallback)
			.unwrap_or(false)
	})
}

pub(super) fn lane_control_mutating_preconditions(report: &Value) -> Vec<Value> {
	report
		.get("runs")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|run| {
			serde_json::json!({
				"projectId": run.get("projectId").cloned().unwrap_or(Value::Null),
				"issueId": run.get("issueId").cloned().unwrap_or(Value::Null),
				"issueIdentifier": run.get("issueIdentifier").cloned().unwrap_or(Value::Null),
				"runId": run.get("runId").cloned().unwrap_or(Value::Null),
				"attemptNumber": run.get("attemptNumber").cloned().unwrap_or(Value::Null),
				"currentTurnId": run.get("turnId").cloned().unwrap_or(Value::Null),
				"laneControlNextAction": run
					.get("laneControlNextAction")
					.cloned()
					.unwrap_or(Value::Null),
				"softInterruptAvailable": run
					.get("softInterruptAvailable")
					.cloned()
					.unwrap_or(Value::Null),
				"hardInterruptAvailable": run
					.get("hardInterruptAvailable")
					.cloned()
					.unwrap_or(Value::Null),
				"hardInterruptRequiresForce": run
					.get("hardInterruptRequiresForce")
					.cloned()
					.unwrap_or(Value::Bool(true)),
				"authority": {
					"inspectedRunId": run.get("runId").cloned().unwrap_or(Value::Null),
					"expectedTurnId": run.get("turnId").cloned().unwrap_or(Value::Null)
				}
			})
		})
		.collect()
}
