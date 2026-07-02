pub(crate) fn protocol_event_counts_as_work_progress(event_type: &str) -> bool {
	let normalized = event_type.to_ascii_lowercase();

	if protocol_event_is_non_work_activity(&normalized) {
		return false;
	}

	normalized.starts_with("turn/")
		|| normalized.starts_with("item/")
		|| normalized == "thread/archive"
		|| normalized.contains("plan")
		|| normalized.contains("diff")
		|| normalized.contains("filechange")
		|| normalized.contains("patch")
		|| normalized.contains("command")
		|| normalized.contains("validation")
		|| normalized.contains("review")
		|| normalized.contains("pull_request")
		|| normalized == "model/response"
}

fn protocol_event_is_non_work_activity(normalized_event_type: &str) -> bool {
	normalized_event_type.starts_with("account/")
		|| normalized_event_type.starts_with("skills/")
		|| normalized_event_type.starts_with("thread/goal/")
		|| normalized_event_type.contains("ratelimit")
		|| normalized_event_type.contains("rate_limit")
		|| normalized_event_type == "thread/status/changed"
		|| normalized_event_type.contains("tokenusage")
		|| matches!(
			normalized_event_type,
			"deprecationnotice"
				| "warning" | "configwarning"
				| "guardianwarning"
				| "model/rerouted"
				| "model/verification"
		)
}
