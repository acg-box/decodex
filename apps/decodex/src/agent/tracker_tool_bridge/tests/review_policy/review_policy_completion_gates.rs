use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
	ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
	ISSUE_TRANSITION_TOOL_NAME, LocalRepoDetails, PullRequestDetails, ReviewHandoffMarker,
	ReviewLevel, TEST_SERVICE_ID, TempDir, TrackerState, TrackerToolBridge, TurnCompletionStatus,
	Value, WorkflowDocument, review_policy,
};

#[test]
fn repair_review_checkpoint_stores_accepted_findings_for_repair_loop() {
	let tracker = FakeTracker::new();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let repair_context = tests::sample_review_repair_context_in(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/242",
	);
	let issue = tests::sample_review_issue();
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		repair_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::repair_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh-context retained repair review accepted one finding"],
			"accepted_findings": review_policy::accepted_review_findings_json(),
			"rejected_findings": [{
				"severity": "info",
				"summary": "Reviewer suggested changing unrelated landing code.",
				"rejection_reason": "Outside this retained repair batch.",
				"evidence": ["The current PR feedback only concerns the tracker-tool bridge."]
			}]
		}),
	);

	assert!(response.success);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &repair_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.phase(), "repair");
	assert_eq!(details["review_contract"]["review_type"], "repair_verification");
	assert_eq!(details["accepted_findings"][0]["summary"], "Accepted reviewer finding");
	assert_eq!(
		details["rejected_findings"][0]["rejection_reason"],
		"Outside this retained repair batch."
	);
}

#[test]
fn stale_review_checkpoint_for_old_head_does_not_stop_new_head() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let mut updated_local_repo = tests::sample_local_repo();

	updated_local_repo.head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(updated_local_repo)]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"blocked",
		&tests::sample_local_repo().head_oid,
		0,
	);

	assert_eq!(
		DynamicToolHandler::classify_turn_completion(&bridge, "continue")
			.expect("a stale checkpoint from an older head should be ignored"),
		TurnCompletionStatus::Continue
	);
}

#[test]
fn review_handoff_requires_a_clean_checkpoint() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(tests::sample_pull_request())]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context_in(temp_dir.path()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/48",
			"summary": "Ready for review."
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("requires a current `handoff` review checkpoint with status `clean`")
	));
}

#[test]
fn review_completion_skips_clean_checkpoint_when_review_gate_disabled() {
	for completion_path in ["handoff", "repair"] {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let tracker = FakeTracker::new();
		let workflow = tests::sample_workflow();

		if completion_path == "handoff" {
			let issue = tests::sample_issue();
			let inspector = FakePullRequestInspector::new(vec![Ok(tests::sample_pull_request())]);
			let local_repo_inspector =
				FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
			let mut review_context = tests::sample_review_context_in(temp_dir.path());

			review_context.review_level = ReviewLevel::Off;

			let bridge = TrackerToolBridge::with_review_handoff_for_test(
				&tracker,
				&issue,
				&workflow,
				review_context,
				&inspector,
				&local_repo_inspector,
			);
			let response = DynamicToolHandler::handle_call(
				&bridge,
				ISSUE_REVIEW_HANDOFF_TOOL_NAME,
				serde_json::json!({
					"pr_url": "https://github.com/hack-ink/decodex/pull/48",
					"summary": "Ready for review."
				}),
			);

			assert!(response.success, "{completion_path} should not require a clean checkpoint");
		} else {
			let review_issue = tests::sample_review_issue();
			let pr_url = "https://github.com/hack-ink/decodex/pull/242";
			let (repair_inspector, repair_local_repo_inspector) =
				review_policy::sample_review_repair_apply_inspectors(pr_url);
			let mut review_context =
				tests::sample_review_repair_context_in(temp_dir.path(), pr_url);

			review_context.review_level = ReviewLevel::Off;

			let bridge = TrackerToolBridge::with_review_repair_for_test(
				&tracker,
				&review_issue,
				&workflow,
				review_context,
				&repair_inspector,
				&repair_local_repo_inspector,
			);
			let response = DynamicToolHandler::handle_call(
				&bridge,
				ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
				serde_json::json!({
					"pr_url": pr_url,
					"summary": "Addressed the requested review changes."
				}),
			);

			assert!(response.success, "{completion_path} should not require a clean checkpoint");
		}
	}
}

#[test]
fn disabled_review_gate_ignores_stale_review_policy_stop_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let mut review_context = tests::sample_review_context_in(temp_dir.path());

	review_context.review_level = ReviewLevel::Off;

	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"findings",
		&tests::sample_local_repo().head_oid,
		3,
	);

	let completion_status = DynamicToolHandler::classify_turn_completion(&bridge, "done")
		.expect("disabled review gate should ignore stale review stop state");

	assert_eq!(completion_status, TurnCompletionStatus::Continue);
}

#[test]
fn review_handoff_rejects_stale_clean_checkpoint_for_previous_head() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let mut updated_local_repo = tests::sample_local_repo();
	let mut updated_pull_request = tests::sample_pull_request();

	updated_local_repo.head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
	updated_pull_request.head_ref_oid = updated_local_repo.head_oid.clone();
	updated_pull_request.url = String::from("https://github.com/hack-ink/decodex/pull/149");

	let review_context = tests::sample_review_context_in(temp_dir.path());
	let inspector = FakePullRequestInspector::new(vec![Ok(updated_pull_request)]);
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(updated_local_repo.clone()), Ok(updated_local_repo)]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"clean",
		&tests::sample_local_repo().head_oid,
		0,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/149",
			"summary": "Ready for review."
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("requires a current `handoff` review checkpoint with status `clean` for the current lane HEAD")
	));
}

#[test]
fn review_handoff_rejects_dirty_worktree_after_clean_checkpoint() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(tests::sample_pull_request())]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(review_policy::sample_dirty_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"clean",
		&tests::sample_local_repo().head_oid,
		0,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/48",
			"summary": "Ready for review."
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("requires a clean committed lane HEAD")
				&& text.contains("record a fresh clean checkpoint")
				&& text.contains("M apps/decodex/src/agent/tracker_tool_bridge/tools.rs")
	));
}

#[test]
fn review_repair_complete_requires_a_clean_checkpoint() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from(pr_url),
	})]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(LocalRepoDetails {
		default_branch: String::from("main"),
		head_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		repository_name: String::from("decodex"),
		repository_owner: String::from("hack-ink"),
		review_blocking_changes: Vec::new(),
	})]);
	let review_context = tests::sample_review_repair_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::bridge_state_store(&bridge)
		.upsert_review_handoff_marker(
			TEST_SERVICE_ID,
			&issue.id,
			&ReviewHandoffMarker::new(
				String::from("pub-618-attempt-2-100"),
				2,
				review_context.branch_name.clone(),
				String::from(pr_url),
				String::from("main"),
				review_context.branch_name.clone(),
				String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			),
		)
		.expect("original review handoff marker should persist");

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Ready for fresh review."
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("requires a current `repair` review checkpoint with status `clean`")
	));
}

#[test]
fn closeout_tool_surface_includes_issue_transition_for_completed_state() {
	let mut issue = tests::sample_review_issue();

	issue
		.team
		.states
		.push(TrackerState { id: String::from("state-done"), name: String::from("Done") });

	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Use the tracker tools.
"#,
	)
	.expect("workflow should parse");
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let temp_dir = TempDir::new().expect("tempdir should create");
	let bridge = TrackerToolBridge::with_run_context(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
	);
	let tool_names = DynamicToolHandler::tool_specs(&bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let transition_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "state": "Done" }),
	);
	let invalid_transition_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "state": "In Progress" }),
	);

	assert!(tool_names.contains(&String::from(ISSUE_TRANSITION_TOOL_NAME)));
	assert!(transition_response.success);
	assert!(!invalid_transition_response.success);
	assert_eq!(tracker.state_updates.borrow().as_slice(), [String::from("state-done")]);
}
