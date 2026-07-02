use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
	ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
	ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, PullRequestDetails, ReviewLevel,
	TempDir, TrackerToolBridge, review_policy,
};

#[test]
fn records_review_handoff_and_applies_it_after_validation() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/42"),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/42"),
		}),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/42",
			"summary": "Implemented the PR-backed review handoff."
		}),
	);

	assert!(response.success);

	tests::assert_review_policy_checkpoint_cleared(&bridge, &issue, &review_context);

	bridge.apply_review_handoff().expect("review handoff should apply");

	assert_eq!(tracker.state_updates.borrow().as_slice(), ["state-review"]);

	let comments = tracker.comments.borrow();

	assert_eq!(comments.len(), 1);
	assert!(comments[0].contains("- pr_url: `https://github.com/hack-ink/decodex/pull/42`"));
	assert!(comments[0].contains("- validation_result: `passed`"));
	assert!(comments[0].contains("- worktree_path: `.worktrees/PUB-618`"));
}

#[test]
fn review_handoff_apply_persists_runtime_handoff_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/142"),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/142"),
		}),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/142",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);

	bridge.apply_review_handoff().expect("review handoff should apply");

	let marker = tests::persisted_review_handoff_marker(&bridge, &issue, &review_context);

	assert_eq!(marker.branch_name(), review_context.branch_name);
	assert_eq!(marker.pr_url(), "https://github.com/hack-ink/decodex/pull/142");
	assert_eq!(marker.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
}

#[test]
fn review_repair_tool_surface_excludes_issue_transition() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let temp_dir = TempDir::new().expect("tempdir should create");
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_repair_context_in(temp_dir.path(), pr_url),
		&inspector,
		&local_repo_inspector,
	);
	let tool_names = DynamicToolHandler::tool_specs(&bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();

	assert!(!tool_names.contains(&String::from(ISSUE_TRANSITION_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_COMMENT_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_LABEL_ADD_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_TERMINAL_FINALIZE_TOOL_NAME)));
}

#[test]
fn review_checkpoint_tool_surface_excludes_closeout() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let review_issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let handoff_pr_inspector = FakePullRequestInspector::new(Vec::new());
	let handoff_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let handoff_bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context_in(temp_dir.path()),
		&handoff_pr_inspector,
		&handoff_repo_inspector,
	);
	let repair_pr_inspector = FakePullRequestInspector::new(Vec::new());
	let repair_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let repair_bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&review_issue,
		&workflow,
		tests::sample_review_repair_context_in(
			temp_dir.path(),
			"https://github.com/hack-ink/decodex/pull/242",
		),
		&repair_pr_inspector,
		&repair_repo_inspector,
	);
	let closeout_bridge = TrackerToolBridge::with_run_context(
		&tracker,
		&review_issue,
		&workflow,
		tests::sample_closeout_context_in(
			temp_dir.path(),
			"https://github.com/hack-ink/decodex/pull/260",
		),
	);
	let handoff_tools = DynamicToolHandler::tool_specs(&handoff_bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let repair_tools = DynamicToolHandler::tool_specs(&repair_bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let closeout_tools = DynamicToolHandler::tool_specs(&closeout_bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();

	assert!(handoff_tools.contains(&String::from(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME)));
	assert!(repair_tools.contains(&String::from(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME)));
	assert!(closeout_tools.contains(&String::from(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME)));
	assert!(handoff_tools.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(repair_tools.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(!closeout_tools.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
}

#[test]
fn review_checkpoint_tool_surface_respects_review_level() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let review_issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let mut review_context = tests::sample_review_context_in(temp_dir.path());
	let mut repair_context = tests::sample_review_repair_context_in(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/242",
	);

	review_context.review_level = ReviewLevel::Off;
	repair_context.review_level = ReviewLevel::Off;

	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);
	let repair_bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&review_issue,
		&workflow,
		repair_context,
		&inspector,
		&local_repo_inspector,
	);
	let tool_names = DynamicToolHandler::tool_specs(&bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let repair_tool_names = DynamicToolHandler::tool_specs(&repair_bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let checkpoint_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"status": "clean",
			"head_sha": "08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			"evidence": []
		}),
	);

	assert!(!tool_names.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_REVIEW_HANDOFF_TOOL_NAME)));
	assert!(!repair_tool_names.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(repair_tool_names.contains(&String::from(ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME)));
	assert!(!checkpoint_response.success);
	assert!(matches!(
		checkpoint_response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("[codex].review = \"off\"")
	));
}

#[test]
fn basic_review_level_does_not_expose_checkpoint_tool() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let mut review_context = tests::sample_review_context_in(temp_dir.path());

	review_context.review_level = ReviewLevel::Basic;

	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&inspector,
		&local_repo_inspector,
	);
	let tool_names = DynamicToolHandler::tool_specs(&bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let checkpoint_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"status": "clean",
			"head_sha": "08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			"evidence": []
		}),
	);

	assert!(!tool_names.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_REVIEW_HANDOFF_TOOL_NAME)));
	assert!(!checkpoint_response.success);
	assert!(matches!(
		checkpoint_response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("[codex].review = \"basic\"")
	));
}

#[test]
fn review_checkpoint_normalizes_matching_short_head_sha_to_full_head() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": &tests::sample_local_repo().head_oid[..7],
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["Closeout and review policy both point at the current lane head."]
		}),
	);

	assert!(response.success);
	assert!(tracker.comments.borrow().is_empty());

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

	assert_eq!(checkpoint.head_sha(), tests::sample_local_repo().head_oid.as_str());
}
