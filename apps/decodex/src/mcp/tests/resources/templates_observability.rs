use serde_json::Value;

use crate::mcp::{
	self,
	tests::support::{self},
};

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

	assert_eq!(
		uri_templates,
		[
			"decodex://decision-contracts/{contract_id}",
			"decodex://projects/{project_id}/status",
			"decodex://projects/{project_id}/status_live",
			"decodex://projects/{project_id}/activity_tail",
			"decodex://projects/{project_id}/lane_inspect/{issue}",
			"decodex://projects/{project_id}/lane-control/{issue}",
			"decodex://projects/{project_id}/runs/{run_id}/events",
			"decodex://projects/{project_id}/runs/{run_id}/protocol_activity",
			"decodex://projects/{project_id}/runs/{run_id}/child_agent_activity",
			"decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics",
			"decodex://projects/{project_id}/pr_review_state",
			"decodex://projects/{project_id}/autonomy",
			"decodex://projects/{project_id}/autonomy/objectives/{objective_id}/current",
			"decodex://projects/{project_id}/autonomy/objectives/{objective_id}/{version}",
			"decodex://projects/{project_id}/autonomy/signals",
			"decodex://projects/{project_id}/autonomy/signals/{signal_id}",
			"decodex://projects/{project_id}/autonomy/proposals",
			"decodex://projects/{project_id}/autonomy/proposals/{proposal_id}",
			"decodex://projects/{project_id}/autonomy/proposals/affected/{namespace}/{value}",
			"decodex://projects/{project_id}/autonomy/evidence",
		]
	);

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
	assert_eq!(combined["live"]["current_lanes"][0]["validation_evidence"]["decision"], "accepted");
	assert!(
		combined["live"]["current_lanes"][0]["validation_evidence"]["changed_surfaces"].is_null()
	);
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
