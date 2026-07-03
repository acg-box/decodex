use crate::mcp::{self, tests::support::observability_review_status_fixture};

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
