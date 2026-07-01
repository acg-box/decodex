#[allow(clippy::wildcard_imports)] use super::*;

pub(in crate::orchestrator) fn protocol_wait_reason_from_child_bucket(
	current_bucket: &str,
) -> String {
	match current_bucket {
		"Model" => String::from("model_execution"),
		"Protocol" => String::from("protocol_activity"),
		_ => String::from("tool_execution"),
	}
}

pub(in crate::orchestrator) fn idle_duration_seconds(
	last_activity_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> Option<i64> {
	last_activity_unix_epoch
		.and_then(|last_activity| now_unix_epoch.checked_sub(last_activity))
		.filter(|idle_for| *idle_for >= 0)
}

pub(in crate::orchestrator) fn max_optional_i64(
	left: Option<i64>,
	right: Option<i64>,
) -> Option<i64> {
	match (left, right) {
		(Some(left), Some(right)) => Some(left.max(right)),
		(Some(value), None) | (None, Some(value)) => Some(value),
		(None, None) => None,
	}
}

pub(in crate::orchestrator) fn format_optional_unix_timestamp(
	unix_epoch: Option<i64>,
) -> Option<String> {
	unix_epoch.and_then(|unix_epoch| {
		OffsetDateTime::from_unix_timestamp(unix_epoch)
			.ok()
			.and_then(|timestamp| timestamp.format(&Rfc3339).ok())
	})
}

pub(in crate::orchestrator) fn format_optional_i64(value: Option<i64>) -> String {
	value.map_or_else(|| String::from("none"), |value| value.to_string())
}
