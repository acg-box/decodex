use serde_json::{self, Value};

use crate::agent::app_server::activity::{
	CHILD_BUCKET_MODEL, CHILD_BUCKET_PROTOCOL, INPUT_TOKEN_KEYS, OUTPUT_TOKEN_KEYS,
	child::{bucket, model::ChildActivityEvent},
	payload,
};

pub(in crate::agent::app_server::activity::child) fn classify_child_activity_event(
	event_type: &str,
	payload: &str,
	active_tool_name: Option<&str>,
) -> ChildActivityEvent {
	let payload_value = serde_json::from_str::<Value>(payload).ok();
	let input_tokens = payload_value
		.as_ref()
		.and_then(|value| payload::find_numeric_field(value, INPUT_TOKEN_KEYS));
	let output_tokens = payload_value
		.as_ref()
		.and_then(|value| payload::find_numeric_field(value, OUTPUT_TOKEN_KEYS));

	match event_type {
		"item/tool/call" => {
			child_tool_call_event(payload_value.as_ref(), input_tokens, output_tokens)
		},
		"item/tool/call/response" => {
			child_tool_response_event(payload_value.as_ref(), active_tool_name, payload)
		},
		"item/completed" => child_item_completed_event(payload_value.as_ref(), payload),
		"item/agentMessage/delta" => ChildActivityEvent {
			event_bucket: CHILD_BUCKET_MODEL.to_owned(),
			event_detail: Some(String::from("agent_message_delta")),
			transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
			transition_detail: Some(String::from("streaming response")),
			tool_name: None,
			tool_call: false,
			tool_output_bytes: None,
			input_tokens,
			output_tokens,
			completed: false,
		},
		"turn/completed" => ChildActivityEvent {
			event_bucket: CHILD_BUCKET_MODEL.to_owned(),
			event_detail: Some(String::from("turn_completed")),
			transition_bucket: None,
			transition_detail: None,
			tool_name: None,
			tool_call: false,
			tool_output_bytes: None,
			input_tokens,
			output_tokens,
			completed: true,
		},
		"thread/status/changed" => ChildActivityEvent {
			event_bucket: CHILD_BUCKET_MODEL.to_owned(),
			event_detail: Some(String::from("thread_status")),
			transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
			transition_detail: Some(String::from("child thread active")),
			tool_name: None,
			tool_call: false,
			tool_output_bytes: None,
			input_tokens,
			output_tokens,
			completed: false,
		},
		other => ChildActivityEvent {
			event_bucket: CHILD_BUCKET_PROTOCOL.to_owned(),
			event_detail: Some(other.to_owned()),
			transition_bucket: None,
			transition_detail: None,
			tool_name: None,
			tool_call: false,
			tool_output_bytes: None,
			input_tokens,
			output_tokens,
			completed: false,
		},
	}
}

fn child_tool_call_event(
	payload_value: Option<&Value>,
	input_tokens: Option<i64>,
	output_tokens: Option<i64>,
) -> ChildActivityEvent {
	let tool_name =
		payload_value.and_then(payload::extract_tool_name).unwrap_or_else(|| String::from("tool"));
	let arguments = payload_value.and_then(payload::extract_tool_arguments);
	let (bucket, detail) = bucket::child_tool_bucket(&tool_name, arguments.as_ref());

	ChildActivityEvent {
		event_bucket: bucket.clone(),
		event_detail: Some(detail.clone()),
		transition_bucket: Some(bucket),
		transition_detail: Some(detail),
		tool_name: Some(tool_name),
		tool_call: true,
		tool_output_bytes: None,
		input_tokens,
		output_tokens,
		completed: false,
	}
}

fn child_tool_response_event(
	payload_value: Option<&Value>,
	active_tool_name: Option<&str>,
	payload: &str,
) -> ChildActivityEvent {
	let tool_name = active_tool_name.unwrap_or("tool").to_owned();
	let (bucket, detail) = bucket::child_tool_bucket(&tool_name, None);
	let output_bytes = payload::tool_output_size(payload_value, payload);

	ChildActivityEvent {
		event_bucket: bucket,
		event_detail: Some(detail),
		transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
		transition_detail: Some(String::from("waiting after tool output")),
		tool_name: Some(tool_name),
		tool_call: false,
		tool_output_bytes: Some(output_bytes),
		input_tokens: payload_value
			.and_then(|value| payload::find_numeric_field(value, INPUT_TOKEN_KEYS)),
		output_tokens: payload_value
			.and_then(|value| payload::find_numeric_field(value, OUTPUT_TOKEN_KEYS)),
		completed: false,
	}
}

fn child_item_completed_event(payload_value: Option<&Value>, payload: &str) -> ChildActivityEvent {
	let item_kind = payload_value
		.and_then(|value| {
			payload::string_at_paths(value, &[&["params", "item", "type"], &["item", "type"]])
		})
		.unwrap_or_else(|| String::from("item"));
	let tool_name = payload_value.and_then(payload::extract_tool_name);
	let input_tokens =
		payload_value.and_then(|value| payload::find_numeric_field(value, INPUT_TOKEN_KEYS));
	let output_tokens =
		payload_value.and_then(|value| payload::find_numeric_field(value, OUTPUT_TOKEN_KEYS));

	if let Some(tool_name) = tool_name
		&& item_kind != "agentMessage"
	{
		let (bucket, detail) = bucket::child_tool_bucket(&tool_name, None);

		return ChildActivityEvent {
			event_bucket: bucket,
			event_detail: Some(detail),
			transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
			transition_detail: Some(String::from("waiting after completed item")),
			tool_name: Some(tool_name),
			tool_call: false,
			tool_output_bytes: Some(payload::tool_output_size(payload_value, payload)),
			input_tokens,
			output_tokens,
			completed: false,
		};
	}

	ChildActivityEvent {
		event_bucket: CHILD_BUCKET_MODEL.to_owned(),
		event_detail: Some(item_kind),
		transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
		transition_detail: Some(String::from("model output")),
		tool_name: None,
		tool_call: false,
		tool_output_bytes: None,
		input_tokens,
		output_tokens,
		completed: false,
	}
}
