#[test]
fn progress_checkpoint_writes_structured_issue_comment() {
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
	assert_eq!(
		record.focus.as_deref(),
		Some("Wire the new execution-state skill into tracker-driven flows.")
	);
	assert_eq!(
		record.next_action.as_deref(),
		Some("Add the issue_progress_checkpoint runtime tool.")
	);
	assert_eq!(record.commit_sha.as_deref(), Some(sample_local_repo().head_oid.as_str()));
	assert_eq!(record.branch.as_deref(), Some("x/decodex-1"));
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

	assert_eq!(record.commit_sha.as_deref(), Some(sample_local_repo().head_oid.as_str()));
}

#[test]
fn progress_checkpoint_retries_do_not_duplicate_same_ledger_event() {
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
}
