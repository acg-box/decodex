use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
			sample_local_repo,
		},
	},
	tracker::records,
};

#[test]
fn progress_checkpoint_preserves_private_payload_and_publishes_projection() {
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
			"phase": "implementing",
				"docs_impact": "none",
			"focus": "Wire the new execution-state skill into tracker-driven flows.",
			"next_action": "Add the issue_progress_checkpoint runtime tool.",
			"blockers": [],
			"evidence": ["Research decision favors Linear-backed execution snapshots."],
			"verification": ["Local inventory of active execution-state boundary references completed."],
			"head_sha": sample_local_repo().head_oid,
			"branch": "x/decodex-1",
			"pr_url": "https://github.com/hack-ink/decodex/pull/12"
		}),
	);

	assert!(response.success);
	assert_eq!(tracker.comments.borrow().len(), 1);

	let comment = tracker.comments.borrow();
	let body = &comment[0];
	let record = records::parse_linear_execution_event_record(body)
		.expect("progress checkpoint should be a Linear execution event");

	assert!(body.starts_with("```json\n{"));
	assert_eq!(record.record_type, records::LINEAR_EXECUTION_EVENT_RECORD_TYPE);
	assert_eq!(record.event_type, "progress_checkpoint");
	assert_eq!(record.phase.as_deref(), Some("implementing"));
	assert_eq!(record.summary.as_deref(), Some("Execution phase: implementing."));
	assert_eq!(record.branch.as_deref(), Some("x/decodex-1"));
	assert_eq!(record.worktree_path.as_deref(), Some(".worktrees/PUB-618"));
	assert_eq!(record.pr_url.as_deref(), Some("https://github.com/hack-ink/decodex/pull/12"));
	assert!(record.focus.is_none());
	assert!(record.next_action.is_none());
	assert!(record.blockers.is_none());
	assert!(record.evidence.is_none());
	assert!(record.verification.is_none());
	assert!(record.commit_sha.is_none());

	let private_events = tests::bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(private_events.len(), 1);
	assert_eq!(private_events[0].event_type(), "progress_checkpoint");
	assert_eq!(
		private_events[0].payload()["focus"],
		serde_json::json!("Wire the new execution-state skill into tracker-driven flows.")
	);
	assert_eq!(
		private_events[0].payload()["next_action"],
		serde_json::json!("Add the issue_progress_checkpoint runtime tool.")
	);
	assert_eq!(private_events[0].payload()["docs_impact"], serde_json::json!("none"));
	assert_eq!(
		private_events[0].payload()["evidence"],
		serde_json::json!(["Research decision favors Linear-backed execution snapshots."])
	);
	assert_eq!(
		private_events[0].payload()["verification"],
		serde_json::json!([
			"Local inventory of active execution-state boundary references completed."
		])
	);
	assert_eq!(
		private_events[0].payload()["head_sha"],
		serde_json::json!(tests::sample_local_repo().head_oid)
	);
}
