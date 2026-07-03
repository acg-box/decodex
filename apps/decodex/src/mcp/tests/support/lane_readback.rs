use serde_json::Value;

pub(in crate::mcp::tests) fn assert_public_lane_inspect_resource(value: &Value) {
	assert_eq!(value["schema"], "decodex.mcp.lane_inspect/1");
	assert_eq!(value["projectId"], "pubfi");
	assert_eq!(value["issue"], "PUB-012");
	assert_eq!(value["matchedRunCount"], 1);

	let run = &value["runs"][0];

	assert_eq!(run["runId"], "run-12");
	assert!(run["status"].as_str().is_some());
	assert!(run["phase"].as_str().is_some());
	assert!(run["currentOperation"].as_str().is_some());
	assert!(run["laneControlNextAction"].as_str().is_some());
	assert!(run["eventCount"].as_i64().is_some());

	assert_no_lane_runtime_identifiers(value);
}

pub(in crate::mcp::tests) fn assert_public_lane_control_readback(value: &Value) {
	assert_eq!(value["schema"], "decodex.mcp.lane_control_readback/1");
	assert_eq!(value["project_id"], "pubfi");
	assert_eq!(value["read_only"], true);

	let run = find_public_lane_control_run(value, "run-12");

	assert_eq!(run["run_id"], "run-12");
	assert!(run["status"].as_str().is_some());
	assert!(run["phase"].as_str().is_some());
	assert!(run["current_operation"].as_str().is_some());
	assert!(run["lane_control_next_action"].as_str().is_some());
	assert!(run["event_count"].as_i64().is_some());

	assert_no_lane_runtime_identifiers(value);
}

pub(in crate::mcp::tests) fn find_public_lane_control_run<'a>(
	value: &'a Value,
	run_id: &str,
) -> &'a Value {
	for key in ["current_lanes", "recent_runs"] {
		if let Some(run) = value[key]
			.as_array()
			.into_iter()
			.flatten()
			.find(|run| run.get("run_id").and_then(Value::as_str) == Some(run_id))
		{
			return run;
		}
	}

	panic!("public lane-control readback should include run {run_id}");
}

pub(in crate::mcp::tests) fn assert_no_lane_runtime_identifiers(value: &Value) {
	let serialized = serde_json::to_string(value).expect("value should serialize");

	for sensitive in [
		"threadId",
		"turnId",
		"threadStatus",
		"processId",
		"processAlive",
		"processLivenessReason",
		"thread_id",
		"turn_id",
		"thread_status",
		"process_id",
		"process_alive",
		"process_liveness_reason",
		"worktreePath",
		"worktree_path",
		"thread-12",
		"turn-12",
	] {
		assert!(!serialized.contains(sensitive), "lane inspect leaked {sensitive}");
	}
}
