use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
		},
	},
	tracker::records,
};

#[test]
fn progress_checkpoint_normalizes_matching_short_head_sha_to_full_head() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context_in(temp_dir.path()),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "closeout",
			"focus": "Finish retained closeout bookkeeping.",
			"next_action": "Record the closeout checkpoint with the live lane head.",
			"blockers": [],
			"evidence": [],
			"head_sha": &tests::sample_local_repo().head_oid[..7]
		}),
	);

	assert!(response.success);
	assert_eq!(tracker.comments.borrow().len(), 1);

	let record = records::parse_linear_execution_event_record(&tracker.comments.borrow()[0])
		.expect("progress checkpoint should be a Linear execution event");

	assert!(record.commit_sha.is_none());

	let private_events = tests::bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(
		private_events[0].payload()["head_sha"],
		serde_json::json!(tests::sample_local_repo().head_oid)
	);
}
