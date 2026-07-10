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
fn preserves_private_events_without_duplicate_projection() {
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
	let arguments = serde_json::json!({
		"phase": "implementing",
		"focus": "Keep duplicate checkpoint writes idempotent.",
		"next_action": "Retry the same tracker write.",
		"blockers": [],
		"evidence": ["The same logical checkpoint is being retried."],
		"head_sha": sample_local_repo().head_oid
	});
	let first = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		arguments.clone(),
	);
	let second =
		DynamicToolHandler::handle_call(&bridge, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, arguments);

	assert!(first.success);
	assert!(second.success);
	assert_eq!(tracker.comments.borrow().len(), 1);

	let private_events = tests::bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(private_events.len(), 2);
}

#[test]
fn progress_checkpoint_public_projection_changes_only_on_material_signal() {
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
	let first = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "implementing",
			"focus": "First private implementation focus.",
			"next_action": "Continue implementation.",
			"blockers": [],
			"evidence": ["First private evidence item."],
			"head_sha": sample_local_repo().head_oid
		}),
	);
	let non_material = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "implementing",
			"focus": "Changed private implementation focus.",
			"next_action": "Changed private next action.",
			"blockers": [],
			"evidence": ["Changed private evidence item."],
			"head_sha": sample_local_repo().head_oid
		}),
	);
	let material = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "verifying",
			"focus": "Verify the implementation.",
			"next_action": "Run tests.",
			"blockers": [],
			"evidence": ["Implementation reached verification."],
			"head_sha": sample_local_repo().head_oid
		}),
	);

	assert!(first.success);
	assert!(non_material.success);
	assert!(material.success);
	assert_eq!(
		tracker.comments.borrow().len(),
		2,
		"private-only changes inside the same public phase must not flood Linear"
	);

	let private_events = tests::bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(private_events.len(), 3);
}

#[test]
fn progress_checkpoint_stores_private_text_but_redacts_public_projection() {
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
			"focus": "Inspected /Users/example/code/private checkout.",
			"next_action": "Continue implementation.",
			"blockers": [],
			"evidence": ["Missing GITHUB_PAT_Y was observed."],
			"head_sha": sample_local_repo().head_oid
		}),
	);

	assert!(response.success);
	assert_eq!(tracker.comments.borrow().len(), 1);

	let body = &tracker.comments.borrow()[0];

	assert!(!body.contains("/Users/example/code/private"));
	assert!(!body.contains("GITHUB_PAT_Y"));

	let record = records::parse_linear_execution_event_record(body)
		.expect("public projection should parse as a Linear execution event");

	assert_eq!(record.phase.as_deref(), Some("implementing"));
	assert_eq!(record.summary.as_deref(), Some("Execution phase: implementing."));
	assert!(record.focus.is_none());
	assert!(record.next_action.is_none());
	assert!(record.evidence.is_none());

	let private_events = tests::bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(
		private_events[0].payload()["focus"],
		serde_json::json!("Inspected /Users/example/code/private checkout.")
	);
	assert_eq!(
		private_events[0].payload()["evidence"],
		serde_json::json!(["Missing GITHUB_PAT_Y was observed."])
	);
}
