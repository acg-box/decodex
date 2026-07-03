use serde_json::Value;

use crate::{
	mcp::{
		self, McpCapabilityProfile, McpContext, ResourceContent,
		tests::support::{self, observability_review_status_fixture},
	},
	state::StateStore,
};

#[test]
fn initialize_exposes_protocol_primitive_capabilities() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
	);
	let response = support::response_at(&responses, 0);
	let result = response.get("result").and_then(Value::as_object).expect("result object");
	let capabilities =
		result.get("capabilities").and_then(Value::as_object).expect("capabilities object");

	assert!(capabilities.contains_key("resources"));
	assert!(capabilities.contains_key("prompts"));
	assert!(capabilities.contains_key("tools"));
	assert!(capabilities.contains_key("logging"));
	assert_eq!(capabilities["experimental"]["decodex"]["capabilityProfile"], "admin");
}

#[test]
fn logging_set_level_is_stdio_compatible() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"logging/setLevel","params":{"level":"debug"}}"#,
	);
	let result = support::response_at(&responses, 0)["result"].as_object().expect("result object");

	assert!(result.is_empty());
}

#[test]
fn resources_list_includes_docs_decisions_and_research_concepts() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
	);
	let resources = support::response_at(&responses, 0)["result"]["resources"]
		.as_array()
		.expect("resources array");
	let uris = resources
		.iter()
		.filter_map(|resource| resource.get("uri").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert!(uris.contains(&"decodex://docs/index"));
	assert!(uris.contains(&"decodex://docs/spec/runtime"));
	assert!(uris.contains(&"decodex://docs/decisions/mcp-gateway"));
	assert!(uris.contains(&"decodex://research/sample-report"));
}

#[test]
fn resources_list_includes_runtime_decision_contracts() {
	let repo = support::test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			support::latent_decision_contract_fixture(),
		)
		.expect("decision contract should persist");

	let responses = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		},
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
	);
	let resources = support::response_at(&responses, 0)["result"]["resources"]
		.as_array()
		.expect("resources array");
	let uris = resources
		.iter()
		.filter_map(|resource| resource.get("uri").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert!(uris.contains(&"decodex://decision-contracts/research-x-loop-contract"));
}

#[test]
fn resources_read_returns_runtime_decision_contract() {
	let repo = support::test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			support::latent_decision_contract_fixture(),
		)
		.expect("decision contract should persist");

	let responses = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		},
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://decision-contracts/research-x-loop-contract"}}"#,
	);
	let contents = support::response_at(&responses, 0)["result"]["contents"]
		.as_array()
		.expect("contents array");
	let text = contents[0]["text"].as_str().expect("text content");
	let content: Value = serde_json::from_str(text).expect("decision contract should be json");

	assert_eq!(content["project_id"], "decodex");
	assert_eq!(content["decision_contract"]["contract_id"], "research-x-loop-contract");
	assert!(content["decision_contract"]["evidence_boundary"]["private_evidence_refs"].is_null());
	assert!(content["decision_contract"]["links"]["execution_program_node_ids"].is_null());
	assert!(!text.contains("research-x-run"));
}

#[test]
fn resources_read_returns_checked_in_doc_text() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://docs/spec/runtime"}}"#,
	);
	let contents = support::response_at(&responses, 0)["result"]["contents"]
		.as_array()
		.expect("contents array");
	let text = contents[0]["text"].as_str().expect("text content");

	assert_eq!(text, "# Runtime\n\nSpec body.\n");
}

#[test]
fn resources_read_returns_checked_in_research_markdown() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://research/sample-report"}}"#,
	);
	let contents = support::response_at(&responses, 0)["result"]["contents"]
		.as_array()
		.expect("contents array");
	let text = contents[0]["text"].as_str().expect("text content");

	assert_eq!(contents[0]["mimeType"], "text/markdown");
	assert_eq!(text, "# Sample Research\n");
}

#[test]
fn observability_sanitizer_strips_private_operator_fields() {
	let mut value = support::sensitive_observability_fixture();

	mcp::sanitize_mcp_observability_value(&mut value);
	support::assert_observability_is_sanitized(&value);
}

#[test]
fn observability_resource_content_strips_private_operator_fields() {
	let content = ResourceContent::mcp_observability_json(
		"decodex://projects/decodex/status",
		support::sensitive_observability_fixture(),
	)
	.expect("observability content should serialize");
	let value: Value = serde_json::from_str(&content.text).expect("content should be json");

	assert_eq!(content.mime_type, "application/json");

	support::assert_observability_is_sanitized(&value);
}

#[test]
fn resources_templates_list_exposes_parameterized_resources() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/templates/list","params":{}}"#,
	);
	let templates = support::response_at(&responses, 0)["result"]["resourceTemplates"]
		.as_array()
		.expect("resource templates array");
	let uri_templates = templates
		.iter()
		.filter_map(|template| template.get("uriTemplate").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert!(uri_templates.contains(&"decodex://docs/spec/{topic}"));
	assert!(uri_templates.contains(&"decodex://research/{concept}"));
	assert!(uri_templates.contains(&"decodex://projects/{project_id}/lane-control/{issue}"));
	assert!(uri_templates.contains(&"decodex://projects/{project_id}/status_live"));
	assert!(uri_templates.contains(&"decodex://projects/{project_id}/activity_tail"));
	assert!(uri_templates.contains(&"decodex://projects/{project_id}/lane_inspect/{issue}"));
	assert!(uri_templates.contains(&"decodex://projects/{project_id}/runs/{run_id}/events"));
	assert!(
		uri_templates.contains(&"decodex://projects/{project_id}/runs/{run_id}/protocol_activity")
	);
	assert!(
		uri_templates
			.contains(&"decodex://projects/{project_id}/runs/{run_id}/child_agent_activity")
	);
	assert!(
		uri_templates
			.contains(&"decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics")
	);
	assert!(uri_templates.contains(&"decodex://projects/{project_id}/pr_review_state"));

	for uri_template in [
		"decodex://projects/{project_id}/runs/{run_id}/events",
		"decodex://projects/{project_id}/runs/{run_id}/protocol_activity",
		"decodex://projects/{project_id}/runs/{run_id}/child_agent_activity",
		"decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics",
	] {
		let template = templates
			.iter()
			.find(|template| {
				template.get("uriTemplate").and_then(Value::as_str) == Some(uri_template)
			})
			.expect("run-scoped resource template should exist");
		let description = template["description"].as_str().expect("description should exist");

		assert!(description.contains("current/recent status snapshot"));
	}
}

#[test]
fn observability_projection_resources_expose_activity_without_private_payloads() {
	let snapshot = support::observability_snapshot_fixture();
	let live = mcp::mcp_status_live_resource(snapshot.clone());
	let activity = mcp::mcp_activity_tail_resource(snapshot.clone());
	let events =
		mcp::mcp_run_resource(&snapshot, "run-1", "events").expect("run events should project");
	let protocol = mcp::mcp_run_resource(&snapshot, "run-1", "protocol_activity")
		.expect("protocol activity should project");
	let child = mcp::mcp_run_resource(&snapshot, "run-1", "child_agent_activity")
		.expect("child-agent activity should project");
	let progress = mcp::mcp_run_resource(&snapshot, "run-1", "progress_diagnostics")
		.expect("progress diagnostics should project");
	let review = mcp::mcp_pr_review_state_resource(snapshot);
	let mut combined = serde_json::json!({
		"live": live,
		"activity": activity,
		"events": events,
		"protocol": protocol,
		"child": child,
		"progress": progress,
		"review": review
	});

	mcp::sanitize_mcp_observability_value(&mut combined);

	assert_eq!(combined["live"]["schema"], "decodex.mcp.status_live/1");
	assert_eq!(combined["live"]["current_lanes"][0]["run_id"], "run-1");
	assert_eq!(combined["live"]["current_lanes"][0]["status"], "running");
	assert_eq!(combined["live"]["current_lanes"][0]["current_operation"], "model_execution");
	assert_eq!(combined["live"]["current_lanes"][0]["event_count"], 6);
	assert_eq!(
		combined["live"]["current_lanes"][0]["lane_control_next_action"],
		"inspect_or_interrupt_orphaned_live_thread"
	);
	assert_eq!(combined["activity"]["activity"][0]["run_id"], "run-1");
	assert_eq!(combined["activity"]["activity"].as_array().expect("activity array").len(), 1);
	assert_eq!(combined["events"]["event_count"], 6);
	assert_eq!(combined["protocol"]["protocol_activity"]["waiting_reason"], "model_execution");
	assert_eq!(
		combined["protocol"]["protocol_activity"]["recent_events"][1]["detail"],
		"redacted_reasoning"
	);
	assert_eq!(
		combined["protocol"]["protocol_activity"]["recent_events"][2]["detail"],
		"redacted_sensitive_detail"
	);
	assert_eq!(
		combined["protocol"]["protocol_activity"]["recent_events"][3]["detail"],
		"redacted_sensitive_detail"
	);
	assert_eq!(
		combined["protocol"]["protocol_activity"]["recent_events"][4]["detail"],
		"redacted_sensitive_detail"
	);
	assert_eq!(
		combined["protocol"]["protocol_activity"]["recent_events"][5]["detail"],
		"redacted_sensitive_detail"
	);
	assert_eq!(
		combined["protocol"]["protocol_activity"]["recent_events"][6]["detail"],
		"redacted_sensitive_detail"
	);
	assert_eq!(combined["child"]["child_agent_activity"]["event_count"], 2);
	assert_eq!(combined["progress"]["progress_diagnostic"], "protocol_only_activity");
	assert_eq!(combined["live"]["current_lanes"][0]["phase_acceptance"]["decision"], "accepted");
	assert!(combined["live"]["current_lanes"][0]["phase_acceptance"]["changed_surfaces"].is_null());
	assert!(combined["live"]["current_lanes"][0]["loop_review"].is_null());
	assert_eq!(combined["review"]["post_review_lanes"][0]["pr_url"], "https://example/pr/1");
	assert!(combined["review"]["post_review_lanes"][0]["branch_name"].is_null());
	assert!(combined["review"]["post_review_lanes"][0]["loop_status"].is_null());
	assert_eq!(
		combined["review"]["current_lane_reviews"].as_array().expect("review array").len(),
		0
	);

	support::assert_no_sensitive_observability_content(&combined);
}

#[test]
fn pr_review_state_ignores_recent_run_reviews_without_current_lane() {
	let snapshot = serde_json::json!({
		"schema": "decodex.mcp.status_resource/1",
		"project_id": "decodex",
		"current_lanes": [],
		"recent_runs": [
			{
				"run_id": "run-stale",
				"issue_id": "issue-stale",
				"issue_identifier": "XY-995",
				"loop_status": {
					"review": {
						"status": "stale_recent_finding"
					}
				}
			}
		],
		"post_review_lanes": []
	});
	let review = mcp::mcp_pr_review_state_resource(snapshot);
	let serialized = serde_json::to_string(&review).expect("review should serialize");

	assert_eq!(review["schema"], "decodex.mcp.pr_review_state/1");
	assert_eq!(review["current_lane_reviews"].as_array().expect("review array").len(), 0);
	assert!(!serialized.contains("stale_recent_finding"));
}

#[test]
fn pr_review_state_includes_object_current_lane_review() {
	let snapshot = serde_json::json!({
		"schema": "decodex.mcp.status_resource/1",
		"project_id": "decodex",
		"current_lanes": [
			{
				"run_id": "run-review",
				"issue_id": "issue-review",
				"issue_identifier": "XY-1095",
				"loop_status": {
					"review": observability_review_status_fixture(
						"private-head-sha",
						"fingerprint-private",
						"stop-fingerprint-private",
						3
					)
				}
			}
		],
		"post_review_lanes": []
	});
	let review = mcp::mcp_pr_review_state_resource(snapshot);
	let current_lane_reviews = review["current_lane_reviews"].as_array().expect("review array");

	assert_eq!(current_lane_reviews.len(), 1);
	assert_eq!(current_lane_reviews[0]["run_id"], "run-review");
	assert_eq!(current_lane_reviews[0]["review"]["status"], "pending");
	assert_eq!(current_lane_reviews[0]["review"]["checkpoint"]["round"], 3);
	assert!(current_lane_reviews[0]["review"]["checkpoint"]["active_fingerprints"].is_null());
}

#[test]
fn mcp_review_surfaces_ignore_null_loop_review_status() {
	let snapshot = serde_json::json!({
		"schema": "decodex.mcp.status_resource/1",
		"project_id": "decodex",
		"current_lanes": [
			{
				"run_id": "run-null-review",
				"issue_id": "issue-null-review",
				"issue_identifier": "XY-1095",
				"loop_status": {
					"review": null
				}
			}
		],
		"post_review_lanes": [
			{
				"project_id": "decodex",
				"issue_id": "issue-null-review",
				"issue_identifier": "XY-1095",
				"loop_status": {
					"review": null
				}
			}
		]
	});
	let review = mcp::mcp_pr_review_state_resource(snapshot.clone());
	let activity = mcp::mcp_run_activity_summary(&snapshot["current_lanes"][0]);
	let post_review_lane = mcp::mcp_public_post_review_lane(&snapshot["post_review_lanes"][0]);

	assert_eq!(review["current_lane_reviews"].as_array().expect("review array").len(), 0);
	assert!(activity["loop_review"].is_null());
	assert!(post_review_lane["loop_review"].is_null());
}

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
