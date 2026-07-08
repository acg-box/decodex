use crate::state::run_activity_marker::record::RunActivityMarkerRecord;

pub(crate) fn parse_run_activity_marker_record(marker_body: &str) -> RunActivityMarkerRecord {
	let mut marker = RunActivityMarkerRecord::default();

	for line in marker_body.lines() {
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};

		match key {
			"run_id" => marker.run_id = Some(value.to_owned()),
			"attempt_number" => marker.attempt_number = value.parse::<i64>().ok(),
			"process_id" => marker.process_id = value.parse::<u32>().ok(),
			"host_boot_id" => marker.host_boot_id = Some(value.to_owned()),
			"process_start_identity" => marker.process_start_identity = Some(value.to_owned()),
			"last_activity_unix_epoch" =>
				marker.last_activity_unix_epoch = value.parse::<i64>().ok(),
			"last_protocol_activity_unix_epoch" =>
				marker.last_protocol_activity_unix_epoch = value.parse::<i64>().ok(),
			"last_progress_unix_epoch" =>
				marker.last_progress_unix_epoch = value.parse::<i64>().ok(),
			"current_operation" => marker.current_operation = Some(value.to_owned()),
			"thread_id" => marker.thread_id = Some(value.to_owned()),
			"turn_id" => marker.turn_id = Some(value.to_owned()),
			"thread_status" => marker.thread_status = Some(value.to_owned()),
			"thread_active_flags" => marker.thread_active_flags = parse_marker_list(value),
			"event_count" => marker.event_count = value.parse::<i64>().ok(),
			"last_event_type" => marker.last_event_type = Some(value.to_owned()),
			"effective_model" => marker.effective_model = Some(value.to_owned()),
			"effective_model_provider" => marker.effective_model_provider = Some(value.to_owned()),
			"effective_cwd" => marker.effective_cwd = Some(value.to_owned()),
			"effective_approval_policy" =>
				marker.effective_approval_policy = Some(value.to_owned()),
			"effective_approvals_reviewer" =>
				marker.effective_approvals_reviewer = Some(value.to_owned()),
			"effective_sandbox_mode" => marker.effective_sandbox_mode = Some(value.to_owned()),
			"child_agent_activity" =>
				marker.child_agent_activity = serde_json::from_str(value).ok(),
			"protocol_activity" => marker.protocol_activity = serde_json::from_str(value).ok(),
			"account" => marker.account = serde_json::from_str(value).ok(),
			"accounts" =>
				if let Ok(accounts) = serde_json::from_str(value) {
					marker.accounts = accounts;
				},
			"retry_budget_attempt_count" =>
				marker.retry_budget_attempt_count = value.parse::<i64>().ok(),
			"retry_kind" => marker.retry_kind = Some(value.to_owned()),
			"retry_ready_at_unix_epoch" =>
				marker.retry_ready_at_unix_epoch = value.parse::<i64>().ok(),
			_ => {},
		}
	}

	marker
}

fn parse_marker_list(value: &str) -> Vec<String> {
	value.split(',').filter(|part| !part.is_empty()).map(str::to_owned).collect()
}
