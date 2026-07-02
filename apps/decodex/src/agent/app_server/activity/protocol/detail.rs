use serde_json::Value;

use crate::agent::app_server::activity::payload;

pub(in crate::agent::app_server::activity::protocol) fn phase_goal_activity_detail(
	payload_value: Option<&Value>,
) -> Option<String> {
	let value = payload_value?;
	let status = payload::string_at_paths(
		value,
		&[&["payload", "status"], &["params", "goal", "status"], &["goal", "status"], &["status"]],
	)?;
	let phase = payload::string_at_paths(value, &[&["phase"], &["payload", "phase"]]);

	Some(match phase {
		Some(phase) => format!("{phase}/{status}"),
		None => status,
	})
}

pub(in crate::agent::app_server::activity::protocol) fn protocol_steer_detail(
	payload_value: Option<&Value>,
) -> Option<String> {
	let value = payload_value?;
	let outcome =
		payload::string_at_paths(value, &[&["outcome"]]).unwrap_or_else(|| String::from("unknown"));
	let expected_turn_id =
		payload::string_at_paths(value, &[&["expectedTurnId"], &["expected_turn_id"]])
			.unwrap_or_else(|| String::from("unknown"));
	let response_turn_id =
		payload::string_at_paths(value, &[&["responseTurnId"], &["response_turn_id"]])
			.unwrap_or_else(|| String::from("none"));

	Some(format!("{outcome}: expected={expected_turn_id}, response={response_turn_id}"))
}

pub(in crate::agent::app_server::activity::protocol) fn warning_or_deprecation_detail(
	payload_value: Option<&Value>,
) -> Option<String> {
	payload_value.and_then(|value| {
		payload::string_at_paths(
			value,
			&[
				&["params", "summary"],
				&["summary"],
				&["params", "message"],
				&["message"],
				&["params", "details"],
				&["details"],
			],
		)
	})
}

pub(in crate::agent::app_server::activity::protocol) fn model_rerouted_detail(
	payload_value: Option<&Value>,
) -> Option<String> {
	let value = payload_value?;
	let from_model = payload::string_at_paths(value, &[&["params", "fromModel"], &["fromModel"]])?;
	let to_model = payload::string_at_paths(value, &[&["params", "toModel"], &["toModel"]])?;
	let reason = payload::string_at_paths(value, &[&["params", "reason"], &["reason"]]);

	Some(match reason {
		Some(reason) => format!("{from_model}->{to_model}/{reason}"),
		None => format!("{from_model}->{to_model}"),
	})
}

pub(in crate::agent::app_server::activity::protocol) fn model_verification_detail(
	payload_value: Option<&Value>,
) -> Option<String> {
	let value = payload_value?;
	let verifications =
		payload::value_at_paths(value, &[&["params", "verifications"], &["verifications"]])?;
	let verification_count = verifications.as_array()?.len();

	Some(format!("{verification_count} verification(s)"))
}

pub(in crate::agent::app_server::activity::protocol) fn token_usage_detail(
	payload_value: Option<&Value>,
) -> Option<String> {
	let value = payload_value?;
	let input_tokens = payload::value_at_paths(
		value,
		&[
			&["params", "tokenUsage", "total", "inputTokens"],
			&["tokenUsage", "total", "inputTokens"],
		],
	)
	.and_then(payload::json_number_to_i64);
	let output_tokens = payload::value_at_paths(
		value,
		&[
			&["params", "tokenUsage", "total", "outputTokens"],
			&["tokenUsage", "total", "outputTokens"],
		],
	)
	.and_then(payload::json_number_to_i64);

	match (input_tokens, output_tokens) {
		(Some(input_tokens), Some(output_tokens)) =>
			Some(format!("input={input_tokens}, output={output_tokens}")),
		(Some(input_tokens), None) => Some(format!("input={input_tokens}")),
		(None, Some(output_tokens)) => Some(format!("output={output_tokens}")),
		(None, None) => None,
	}
}

pub(in crate::agent::app_server::activity::protocol) fn protocol_account_detail(
	payload_value: Option<&Value>,
) -> Option<String> {
	let value = payload_value?;
	let plan = payload::string_at_paths(
		value,
		&[
			&["params", "planType"],
			&["params", "chatgptPlanType"],
			&["params", "rateLimits", "planType"],
			&["planType"],
			&["chatgptPlanType"],
			&["rateLimits", "planType"],
		],
	);
	let status = payload::string_at_paths(
		value,
		&[
			&["params", "status"],
			&["params", "refreshStatus"],
			&["params", "rateLimits", "rateLimitReachedType"],
			&["status"],
			&["refreshStatus"],
			&["rateLimits", "rateLimitReachedType"],
		],
	);

	match (plan, status) {
		(Some(plan), Some(status)) => Some(format!("{plan}/{status}")),
		(Some(plan), None) => Some(plan),
		(None, Some(status)) => Some(status),
		(None, None) => None,
	}
}
