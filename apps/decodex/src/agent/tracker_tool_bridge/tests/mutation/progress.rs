#[test]
fn progress_checkpoint_preserves_private_payload_and_publishes_projection() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_context_in(temp_dir.path()),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "implementing",
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

	let private_events = bridge_state_store(&bridge)
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
	assert_eq!(
		private_events[0].payload()["evidence"],
		serde_json::json!(["Research decision favors Linear-backed execution snapshots."])
	);
	assert_eq!(
		private_events[0].payload()["verification"],
		serde_json::json!(["Local inventory of active execution-state boundary references completed."])
	);
	assert_eq!(
		private_events[0].payload()["head_sha"],
		serde_json::json!(sample_local_repo().head_oid)
	);
}

#[test]
fn blocked_progress_checkpoint_requires_concrete_blocker() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "blocked",
			"focus": "Unblock closeout.",
			"next_action": "Wait for a blocker to be clarified.",
			"blockers": [],
			"evidence": []
		}),
	);

	assert!(!response.success);
	assert!(
		response
			.content_items
			.iter()
			.any(|item| matches!(item, DynamicToolContentItem::InputText { text } if text.contains("requires at least one blocker")))
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn progress_checkpoint_rejects_stale_head_sha() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_context_in(temp_dir.path()),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "implementing",
			"focus": "Keep execution state tied to the current lane head.",
			"next_action": "Reject stale checkpoint writes.",
			"blockers": [],
			"evidence": [],
			"head_sha": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
		}),
	);

	assert!(!response.success);
	assert!(
		response
			.content_items
			.iter()
			.any(|item| matches!(item, DynamicToolContentItem::InputText { text } if text.contains("does not match the current lane HEAD")))
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn progress_checkpoint_normalizes_matching_short_head_sha_to_full_head() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_context_in(temp_dir.path()),
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
			"head_sha": &sample_local_repo().head_oid[..7]
		}),
	);

	assert!(response.success);
	assert_eq!(tracker.comments.borrow().len(), 1);

	let record = records::parse_linear_execution_event_record(&tracker.comments.borrow()[0])
		.expect("progress checkpoint should be a Linear execution event");

	assert!(record.commit_sha.is_none());

	let private_events = bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(
		private_events[0].payload()["head_sha"],
		serde_json::json!(sample_local_repo().head_oid)
	);
}

#[test]
fn progress_checkpoint_retries_preserve_private_events_without_duplicate_public_projection() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_context_in(temp_dir.path()),
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

	let private_events = bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(private_events.len(), 2);
}

#[test]
fn progress_checkpoint_public_projection_changes_only_on_material_signal() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_context_in(temp_dir.path()),
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

	let private_events = bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private checkpoint events should list");

	assert_eq!(private_events.len(), 3);
}

#[test]
fn progress_checkpoint_stores_private_text_but_redacts_public_projection() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_context_in(temp_dir.path()),
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

	let private_events = bridge_state_store(&bridge)
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
