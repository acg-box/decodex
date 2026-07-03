use crate::mcp::{
	self, McpCapabilityProfile, McpContext,
	tests::support::{self},
};

#[test]
fn resources_read_exposes_bounded_live_activity_and_recent_run_readback() {
	let repo = support::test_repo();
	let _runtime_home_guard = support::isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	support::seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: Some(config_path.clone()),
		project_id: Some(String::from("pubfi")),
		state_store: None,
	};
	let responses = support::run_stdio_with_context(
			context,
			&[
				r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/status_live"}}"#,
				r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/activity_tail"}}"#,
				r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/runs/run-12/events"}}"#,
				r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/runs/run-01/events"}}"#,
				r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/lane_inspect/PUB-012"}}"#,
				r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/lane-control/PUB-012"}}"#,
				r#"{"jsonrpc":"2.0","id":7,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/lane-control"}}"#,
				r#"{"jsonrpc":"2.0","id":8,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/pr_review_state"}}"#,
				r#"{"jsonrpc":"2.0","id":9,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/runs/run-12/protocol_activity"}}"#,
			]
			.join("\n"),
		);
	let status_live = support::resource_response_json(&responses, 0);
	let activity_tail = support::resource_response_json(&responses, 1);
	let run_events = support::resource_response_json(&responses, 2);
	let hidden_run_error = support::response_error(&responses, 3);
	let lane_inspect = support::resource_response_json(&responses, 4);
	let lane_control_issue = support::resource_response_json(&responses, 5);
	let lane_control = support::resource_response_json(&responses, 6);
	let pr_review_state = support::resource_response_json(&responses, 7);
	let protocol_activity = support::resource_response_json(&responses, 8);

	assert_eq!(status_live["schema"], "decodex.mcp.status_live/1");
	assert_eq!(activity_tail["schema"], "decodex.mcp.activity_tail/1");
	assert_eq!(
		activity_tail["activity"].as_array().expect("activity array").len(),
		mcp::DEFAULT_MCP_STATUS_LIMIT
	);
	assert_eq!(run_events["schema"], "decodex.mcp.run_events/1");
	assert_eq!(run_events["run_id"], "run-12");
	assert_eq!(run_events["event_count"], 6);
	assert_eq!(hidden_run_error["code"], mcp::RESOURCE_NOT_FOUND_CODE);

	support::assert_public_lane_inspect_resource(&lane_inspect);
	support::assert_public_lane_inspect_resource(&lane_control_issue);
	support::assert_public_lane_control_readback(&lane_control);

	assert_eq!(pr_review_state["schema"], "decodex.mcp.pr_review_state/1");

	let current_lane_reviews =
		pr_review_state["current_lane_reviews"].as_array().expect("review array");

	assert!(
		current_lane_reviews.is_empty(),
		"unexpected current lane reviews: {current_lane_reviews:?}"
	);
	assert_eq!(protocol_activity["schema"], "decodex.mcp.protocol_activity/1");
	assert_eq!(protocol_activity["run_id"], "run-12");
	assert!(
		serde_json::to_string(&protocol_activity)
			.expect("protocol activity should serialize")
			.contains("redacted_sensitive_detail")
	);

	support::assert_no_sensitive_observability_content(&serde_json::json!({
		"status_live": status_live,
		"activity_tail": activity_tail,
		"lane_inspect": lane_inspect,
		"lane_control_issue": lane_control_issue,
		"lane_control": lane_control,
		"pr_review_state": pr_review_state,
		"protocol_activity": protocol_activity
	}));
}

#[test]
fn streamable_http_resources_read_exposes_observability_resources() {
	let repo = support::test_repo();
	let _runtime_home_guard = support::isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	support::seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: Some(config_path),
		project_id: Some(String::from("pubfi")),
		state_store: None,
	};
	let mut handler =
		support::http_handler_with_context(context, McpCapabilityProfile::Observe, Vec::new());
	let initialize = support::run_http(
		&mut handler,
		support::http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193")],
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
		),
	);
	let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
	let status_live = support::http_resource_read_json(
		&mut handler,
		&session_id,
		2,
		"decodex://projects/pubfi/status_live",
	);
	let activity_tail = support::http_resource_read_json(
		&mut handler,
		&session_id,
		3,
		"decodex://projects/pubfi/activity_tail",
	);
	let pr_review_state = support::http_resource_read_json(
		&mut handler,
		&session_id,
		4,
		"decodex://projects/pubfi/pr_review_state",
	);
	let lane_inspect = support::http_resource_read_json(
		&mut handler,
		&session_id,
		5,
		"decodex://projects/pubfi/lane_inspect/PUB-012",
	);
	let lane_control_issue = support::http_resource_read_json(
		&mut handler,
		&session_id,
		6,
		"decodex://projects/pubfi/lane-control/PUB-012",
	);
	let lane_control = support::http_resource_read_json(
		&mut handler,
		&session_id,
		7,
		"decodex://projects/pubfi/lane-control",
	);
	let protocol_activity = support::http_resource_read_json(
		&mut handler,
		&session_id,
		8,
		"decodex://projects/pubfi/runs/run-12/protocol_activity",
	);
	let hidden_run = support::http_json_rpc(
		&mut handler,
		&session_id,
		r#"{"jsonrpc":"2.0","id":9,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/runs/run-01/events"}}"#,
	);

	assert_eq!(status_live["schema"], "decodex.mcp.status_live/1");
	assert_eq!(activity_tail["schema"], "decodex.mcp.activity_tail/1");
	assert_eq!(
		activity_tail["activity"].as_array().expect("activity array").len(),
		mcp::DEFAULT_MCP_STATUS_LIMIT
	);
	assert_eq!(pr_review_state["schema"], "decodex.mcp.pr_review_state/1");

	support::assert_public_lane_inspect_resource(&lane_inspect);
	support::assert_public_lane_inspect_resource(&lane_control_issue);
	support::assert_public_lane_control_readback(&lane_control);

	assert_eq!(protocol_activity["schema"], "decodex.mcp.protocol_activity/1");

	let current_lane_reviews =
		pr_review_state["current_lane_reviews"].as_array().expect("review array");

	assert!(
		current_lane_reviews.is_empty(),
		"unexpected current lane reviews: {current_lane_reviews:?}"
	);
	assert!(
		serde_json::to_string(&protocol_activity)
			.expect("protocol activity should serialize")
			.contains("redacted_sensitive_detail")
	);
	assert_eq!(hidden_run["error"]["code"], mcp::RESOURCE_NOT_FOUND_CODE);

	support::assert_no_sensitive_observability_content(&serde_json::json!({
		"status_live": status_live,
		"activity_tail": activity_tail,
		"pr_review_state": pr_review_state,
		"lane_inspect": lane_inspect,
		"lane_control_issue": lane_control_issue,
		"lane_control": lane_control,
		"protocol_activity": protocol_activity
	}));
}
