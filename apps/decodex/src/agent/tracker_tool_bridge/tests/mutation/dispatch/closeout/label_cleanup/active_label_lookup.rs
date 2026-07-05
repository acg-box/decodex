use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
		},
	},
	tracker::{self, TrackerLabel, TrackerState},
};

#[test]
fn closeout_clear_uses_server_team_label_lookup_for_active_label_removal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	completed_issue
		.labels
		.push(TrackerLabel { id: String::from("label-active"), name: active_label.clone() });
	completed_issue
		.labels
		.push(TrackerLabel { id: String::from("label-queued"), name: queue_label.clone() });
	completed_issue.team.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::with_refresh_snapshots(vec![
		vec![completed_issue.clone()],
		vec![completed_issue.clone()],
	])
	.with_team_label_lookup_id(&completed_issue.team.id, &active_label, "label-active");
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_closeout_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
		"pr_url": pr_url,
		"summary": "Merged the approved lane and finished closeout."
		}),
	);

	tests::seed_docs_impact_checkpoint(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"closeout",
		&tests::sample_local_repo().head_oid,
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "closeout" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	bridge.clear_closeout_issue_scope().expect(
		"closeout cleanup should resolve the active label id server-side when team labels paginate",
	);

	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[vec![String::from("label-active")], vec![String::from("label-queued")],]
	);
	assert!(tracker.state_updates.borrow().is_empty());
}
