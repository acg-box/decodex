use std::time::{Duration, Instant};

use serde_json::Value;

use crate::agent::json_rpc::{JsonRpcMessage, JsonRpcNotification, WireMessage};

pub(in crate::agent::app_server) fn remaining_idle_budget(
	last_activity_at: Instant,
	now: Instant,
	timeout: Duration,
) -> Option<Duration> {
	timeout.checked_sub(now.saturating_duration_since(last_activity_at))
}

pub(in crate::agent::app_server) fn message_type(message: &WireMessage) -> &str {
	match &message.message {
		JsonRpcMessage::Notification(notification) => notification.method.as_str(),
		JsonRpcMessage::Request(request) => request.method.as_str(),
		JsonRpcMessage::Response(_) => "json-rpc/response",
		JsonRpcMessage::Error(_) => "json-rpc/error",
	}
}

pub(in crate::agent::app_server) fn targets_thread(
	message: &WireMessage,
	target_thread_id: Option<&str>,
) -> bool {
	let Some(target_thread_id) = target_thread_id else {
		return true;
	};

	match &message.message {
		JsonRpcMessage::Notification(notification) => thread_id_from_notification(notification)
			.is_none_or(|thread_id| thread_id == target_thread_id),
		JsonRpcMessage::Request(request) => thread_id_from_value(&request.params)
			.is_none_or(|thread_id| thread_id == target_thread_id),
		JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => true,
	}
}

pub(super) fn thread_id_from_notification(notification: &JsonRpcNotification) -> Option<&str> {
	thread_id_from_value(&notification.params)
}

pub(in crate::agent::app_server) fn thread_id_from_value(value: &Value) -> Option<&str> {
	value
		.get("threadId")
		.and_then(Value::as_str)
		.or_else(|| value.get("thread").and_then(|thread| thread.get("id")).and_then(Value::as_str))
}

pub(in crate::agent::app_server) fn turn_id_from_value(value: &Value) -> Option<&str> {
	value
		.get("turnId")
		.and_then(Value::as_str)
		.or_else(|| value.get("turn").and_then(|turn| turn.get("id")).and_then(Value::as_str))
}
