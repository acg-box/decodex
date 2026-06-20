fn sample_review_repair_apply_inspectors(
	pr_url: &str,
) -> (FakePullRequestInspector, FakeLocalRepoInspector) {
	let inspector = FakePullRequestInspector::new(vec![
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from(pr_url),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from(pr_url),
		}),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(LocalRepoDetails {
			default_branch: String::from("main"),
			head_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			repository_name: String::from("decodex"),
			repository_owner: String::from("hack-ink"),
			review_blocking_changes: Vec::new(),
		}),
		Ok(LocalRepoDetails {
			default_branch: String::from("main"),
			head_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			repository_name: String::from("decodex"),
			repository_owner: String::from("hack-ink"),
			review_blocking_changes: Vec::new(),
		}),
	]);

	(inspector, local_repo_inspector)
}

fn review_checks_json() -> Value {
	serde_json::json!({
		"intended_behavior": "Checked the implementation against the issue requirements.",
		"regression_risk": "Checked shared runtime regression risk for the touched path.",
		"missing_tests": "Checked whether the current change needs additional tests.",
		"docs_config_drift": "Checked docs and config drift for the runtime behavior change.",
		"migration_fallout": "Checked additive runtime-store migration fallout.",
		"operator_facing_fallout": "Checked Linear and operator-facing fallout.",
		"loop_decision_contract": "Compared the change against the accepted Loop/Decision Contract and found no mismatch."
	})
}

fn handoff_review_contract_json() -> Value {
	review_contract_json("full_current_head_review")
}

fn repair_review_contract_json() -> Value {
	review_contract_json("repair_verification")
}

fn review_contract_json(review_type: &str) -> Value {
	serde_json::json!({
		"workflow_policy_source": "registered_project_workflow",
		"review_type": review_type,
		"risk_tier": "localized",
		"objective": "Review the current committed lane head against the accepted issue contract.",
		"scope": ["Current committed lane diff and directly owned behavior."],
		"non_goals": ["Do not widen into unrelated cleanup or unowned product direction."],
		"required_checks": ["Intended behavior, regression risk, tests, docs/config drift, migration fallout, operator-facing fallout, and Loop/Decision Contract alignment."],
		"allowed_expansion_triggers": ["Safety, authority-boundary, data-loss, security, live-mutation, public-API, migration, or operator-facing regression."],
		"validation_evidence": ["Repo-native validation was rerun for the committed lane head before review."]
	})
}

fn accepted_review_findings_json() -> Value {
	accepted_review_findings_with_summary_json(
		"Accepted reviewer finding",
		"Repair the accepted issue before requesting another review checkpoint.",
		1,
	)
}

fn accepted_review_findings_with_summary_json(
	summary: &str,
	guidance: &str,
	line: u64,
) -> Value {
	serde_json::json!([{
		"severity": "medium",
		"summary": summary,
		"evidence": ["The reviewer evidence points at the current lane head."],
		"file": "apps/decodex/src/agent/tracker_tool_bridge/tools.rs",
		"line": line,
		"guidance": guidance
	}])
}

fn accepted_review_findings_for_status_json(status: &str) -> Value {
	if status == "findings" {
		accepted_review_findings_json()
	} else {
		serde_json::json!([])
	}
}

fn route_only_review_route_json(route: &str) -> Value {
	serde_json::json!([{
		"route": route,
		"severity": "medium",
		"risk_tier": "medium",
		"summary": "Review signal is routed outside current repair.",
		"evidence": ["The reviewer signal was checked against the current lane head."],
		"resolver": "architecture",
		"next_action": "Record the routed review signal without mutating the current repair."
	}])
}

fn sample_dirty_local_repo() -> LocalRepoDetails {
	let mut local_repo = sample_local_repo();

	local_repo.review_blocking_changes = vec![
		String::from("M apps/decodex/src/agent/tracker_tool_bridge/tools.rs"),
		String::from("?? apps/decodex/src/agent/new_review_surface.rs"),
	];

	local_repo
}

fn submit_findings_review_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	evidence: &str,
) -> DynamicToolCallResponse {
	submit_findings_review_checkpoint_with_findings(
		bridge,
		evidence,
		accepted_review_findings_json(),
	)
}

fn submit_findings_review_checkpoint_with_findings(
	bridge: &TrackerToolBridge<'_>,
	evidence: &str,
	accepted_findings: Value,
) -> DynamicToolCallResponse {
	DynamicToolHandler::handle_call(
		bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": [evidence],
			"accepted_findings": accepted_findings
		}),
	)
}

fn seed_review_repair_apply_state(
	state_store: &StateStore,
	review_context: &ReviewHandoffContext,
	issue_id: &str,
	pr_url: &str,
	external_round_count: i64,
) {
	let review_handoff = ReviewHandoffMarker::new(
		String::from("pub-618-attempt-2-100"),
		2,
		review_context.branch_name.clone(),
		String::from(pr_url),
		String::from("main"),
		review_context.branch_name.clone(),
		String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
	);

	state_store
		.upsert_review_handoff_marker(&review_context.service_id, issue_id, &review_handoff)
		.expect("original review handoff marker should persist");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: &review_context.service_id,
			issue_id,
			run_id: &review_context.run_id,
			attempt_number: review_context.attempt_number,
			phase: "repair",
			review_level: review_context.review_level.as_str(),
			status: "clean",
			head_sha: "18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("repair review checkpoint should persist");
	state_store
		.upsert_review_orchestration_marker(
			&review_context.service_id,
			issue_id,
			&ReviewOrchestrationMarker::new(
				review_handoff.run_id().to_owned(),
				review_handoff.attempt_number(),
				review_handoff.branch_name().to_owned(),
				pr_url.to_owned(),
				String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
				"repair_required",
				Some(91),
				Some(1_763_600_000),
				Some(0),
				0,
				external_round_count,
				None,
			),
		)
		.expect("review orchestration marker should persist");
}

#[test]
fn records_review_handoff_and_applies_it_after_validation() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
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
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(sample_local_repo()), Ok(sample_local_repo())]);
	let review_context = sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	write_clean_review_checkpoint(&bridge, &issue, &review_context);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/42",
			"summary": "Implemented the PR-backed review handoff."
		}),
	);

	assert!(response.success);

	assert_review_policy_checkpoint_cleared(&bridge, &issue, &review_context);

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
	let issue = sample_issue();
	let workflow = sample_workflow();
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
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(sample_local_repo()), Ok(sample_local_repo())]);
	let review_context = sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	write_clean_review_checkpoint(&bridge, &issue, &review_context);

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

	let marker = persisted_review_handoff_marker(&bridge, &issue, &review_context);

	assert_eq!(marker.branch_name(), review_context.branch_name);
	assert_eq!(marker.pr_url(), "https://github.com/hack-ink/decodex/pull/142");
	assert_eq!(marker.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
}

#[test]
fn review_repair_tool_surface_excludes_issue_transition() {
	let tracker = FakeTracker::new();
	let issue = sample_review_issue();
	let workflow = sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let temp_dir = TempDir::new().expect("tempdir should create");
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_repair_context_in(temp_dir.path(), pr_url),
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
	let issue = sample_issue();
	let review_issue = sample_review_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let handoff_pr_inspector = FakePullRequestInspector::new(Vec::new());
	let handoff_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let handoff_bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_context_in(temp_dir.path()),
		&handoff_pr_inspector,
		&handoff_repo_inspector,
	);
	let repair_pr_inspector = FakePullRequestInspector::new(Vec::new());
	let repair_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let repair_bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&review_issue,
		&workflow,
		sample_review_repair_context_in(
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
		sample_closeout_context_in(temp_dir.path(), "https://github.com/hack-ink/decodex/pull/260"),
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
	let issue = sample_issue();
	let review_issue = sample_review_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let mut review_context = sample_review_context_in(temp_dir.path());
	let mut repair_context = sample_review_repair_context_in(
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
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let mut review_context = sample_review_context_in(temp_dir.path());

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
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
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
			"head_sha": &sample_local_repo().head_oid[..7],
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["Closeout and review policy both point at the current lane head."]
		}),
	);

	assert!(response.success);
	assert!(tracker.comments.borrow().is_empty());

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

	assert_eq!(checkpoint.head_sha(), sample_local_repo().head_oid.as_str());
}

#[test]
fn independent_review_checkpoint_requires_structured_fresh_context_payload() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);

	for (payload, expected_error) in [
		(
			serde_json::json!({
				"status": "clean",
				"head_sha": sample_local_repo().head_oid,
				"checks": review_checks_json(),
				"evidence": ["review evidence"]
			}),
			"requires `reviewer`",
		),
		(
			serde_json::json!({
				"reviewer": "self_review",
				"status": "clean",
				"head_sha": sample_local_repo().head_oid,
				"checks": review_checks_json(),
				"evidence": ["review evidence"]
			}),
			"reviewer must be `independent_fresh_context`",
		),
		(
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": sample_local_repo().head_oid,
				"review_contract": handoff_review_contract_json(),
				"evidence": ["review evidence"]
			}),
			"requires `checks`",
		),
		(
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": sample_local_repo().head_oid,
				"review_contract": handoff_review_contract_json(),
				"checks": review_checks_json(),
				"evidence": []
			}),
			"requires `evidence`",
		),
		(
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "findings",
				"head_sha": sample_local_repo().head_oid,
				"review_contract": handoff_review_contract_json(),
				"checks": review_checks_json(),
				"evidence": ["review evidence"],
				"accepted_findings": [{
					"severity": "medium",
					"summary": "Accepted reviewer finding",
					"evidence": [],
					"guidance": "Repair the accepted issue before requesting another checkpoint."
				}]
			}),
			"requires `accepted_findings.evidence`",
		),
		(
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": sample_local_repo().head_oid,
				"review_contract": handoff_review_contract_json(),
				"checks": review_checks_json(),
				"evidence": ["review evidence"],
				"rejected_findings": [{
					"severity": "unknown",
					"summary": "Rejected reviewer finding",
					"rejection_reason": "Not actionable after validation.",
					"evidence": ["Reviewer evidence was stale."]
				}]
			}),
			"`rejected_findings.severity` must be",
		),
	] {
		let response =
			DynamicToolHandler::handle_call(&bridge, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, payload);

		assert!(!response.success);
		assert!(matches!(
			response.content_items.as_slice(),
			[DynamicToolContentItem::InputText { text }] if text.contains(expected_error)
		));
	}
}

#[test]
fn independent_review_checkpoint_requires_review_contract() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": sample_local_repo().head_oid,
			"checks": review_checks_json(),
			"evidence": ["review evidence"]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }] if text.contains("requires `review_contract`")
	));
}

#[test]
fn independent_review_checkpoint_clean_persists_structured_payload() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
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
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh reviewer read the issue contract, current diff, and HEAD"],
			"rejected_findings": [{
				"severity": "low",
				"summary": "The reviewer asked for a migration note, but no schema or data migration changed.",
				"rejection_reason": "Not actionable after checking the current diff and docs.",
				"evidence": ["Only runtime review checkpoint metadata changed."],
				"file": "apps/decodex/src/agent/tracker_tool_bridge/tools.rs",
				"line": 1
			}]
		}),
	);

	assert!(response.success);

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.status(), "clean");
	assert_eq!(details["reviewer"], "independent_fresh_context");
	assert_eq!(
		details["review_contract"]["workflow_policy_source"],
		"registered_project_workflow"
	);
	assert_eq!(details["review_contract"]["review_type"], "full_current_head_review");
	assert_eq!(details["reviewed_head"]["head_sha"], sample_local_repo().head_oid);
	assert_eq!(details["reviewed_head"]["head_tree_oid"], sample_local_repo().head_tree_oid);
	assert_eq!(details["reviewed_head"]["review_worktree_clean"], true);
	assert!(
		details["review_contract_hash"]
			.as_str()
			.is_some_and(|hash| hash.starts_with("review_contract:"))
	);
	assert_eq!(
		details["checks"]["loop_decision_contract"],
		"Compared the change against the accepted Loop/Decision Contract and found no mismatch."
	);
	assert_eq!(details["accepted_findings"].as_array().expect("accepted findings array").len(), 0);
	assert_eq!(details["rejected_findings"][0]["rejection_reason"], "Not actionable after checking the current diff and docs.");
	assert_eq!(details["finding_routes"][0]["route"], "reviewer_rubric_gap");
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["route"], "reviewer_rubric_gap");
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["count"], 1);

	let events = bridge_state_store(&bridge)
		.list_private_execution_events_for_run_attempt(
			&review_context.service_id,
			&review_context.run_id,
			review_context.attempt_number,
		)
		.expect("private review evidence should read");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "review_checkpoint");
	assert_eq!(events[0].payload()["review"]["reviewer"], "independent_fresh_context");
	assert_eq!(events[0].payload()["route_counts"][0]["route"], "reviewer_rubric_gap");
}

#[test]
fn review_checkpoint_rejects_review_blocking_local_changes() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_dirty_local_repo())]);
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
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["review tried to bind a dirty worktree"]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires a clean committed lane HEAD")
				&& text.contains("M apps/decodex/src/agent/tracker_tool_bridge/tools.rs")
				&& text.contains("?? apps/decodex/src/agent/new_review_surface.rs")
	));
	assert!(
		bridge_state_store(&bridge)
			.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
				project_id: &review_context.service_id,
				issue_id: &issue.id,
				phase: "handoff",
				review_level: review_context.review_level.as_str(),
				head_sha: &sample_local_repo().head_oid,
			})
			.expect("artifact lookup should succeed")
			.is_none(),
		"dirty checkpoint attempts must not persist reusable review evidence"
	);
}

#[test]
fn independent_review_checkpoint_findings_store_accepted_repair_guidance() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(sample_local_repo()), Ok(sample_local_repo())]);
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
			"status": "findings",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh reviewer found one accepted repair item"],
			"accepted_findings": accepted_review_findings_json()
		}),
	);

	assert!(response.success);
	assert_eq!(
		DynamicToolHandler::classify_turn_completion(&bridge, "continue")
			.expect("first accepted findings round should continue"),
		TurnCompletionStatus::Continue
	);

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.nonclean_rounds(), 1);
	assert_eq!(details["accepted_findings"][0]["severity"], "medium");
	assert_eq!(details["accepted_findings"][0]["kind"], "accepted_finding");
	assert_eq!(details["accepted_findings"][0]["line_range"]["start"], 1);
	assert!(
		details["accepted_findings"][0]["fingerprint"]
			.as_str()
			.is_some_and(|fingerprint| fingerprint.starts_with("review_finding:"))
	);
	assert_eq!(
		details["accepted_findings"][0]["guidance"],
		"Repair the accepted issue before requesting another review checkpoint."
	);
	assert_eq!(details["finding_routes"][0]["route"], "current_blocker");
	assert_eq!(
		details["finding_routes"][0]["finding_fingerprint"],
		details["accepted_findings"][0]["fingerprint"]
	);
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["route"], "current_blocker");
}

#[test]
fn review_checkpoint_rejected_finding_is_non_actionable_and_can_handoff_cleanly() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(sample_pull_request())]);
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(sample_local_repo()), Ok(sample_local_repo())]);
	let review_context = sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["only rejected non-actionable feedback remained"],
			"rejected_findings": [{
				"severity": "low",
				"summary": "The reviewer requested a migration test.",
				"rejection_reason": "No migration path changed in the current diff.",
				"evidence": ["The runtime store column is additive and defaults existing rows."],
				"file": "apps/decodex/src/state/internal.rs",
				"line": 1
			}]
		}),
	);

	assert!(response.success);

	let handoff_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/48",
			"summary": "Rejected non-actionable review feedback and prepared handoff."
		}),
	);

	assert!(handoff_response.success);

	assert_review_policy_checkpoint_cleared(&bridge, &issue, &review_context);
}

#[test]
fn clean_review_checkpoint_records_non_current_routes_without_churn() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(sample_local_repo()), Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	for _round in 0..2 {
		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": sample_local_repo().head_oid,
				"review_contract": handoff_review_contract_json(),
				"checks": review_checks_json(),
				"evidence": ["fresh reviewer found only non-current follow-up work"],
				"finding_routes": route_only_review_route_json("follow_up")
			}),
		);

		assert!(response.success);
	}

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.status(), "clean");
	assert_eq!(checkpoint.nonclean_rounds(), 0);
	assert_eq!(
		details["finding_policy"]["active_fingerprints"]
			.as_array()
			.expect("active fingerprints should be an array")
			.len(),
		0
	);
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["route"], "follow_up");
}

#[test]
fn review_checkpoint_rejects_high_risk_invalid_route() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh reviewer disputed a severe live-mutation risk"],
			"finding_routes": [{
				"route": "invalid_or_unsubstantiated",
				"severity": "high",
				"risk_tier": "high",
				"summary": "Reviewer alleged a high-risk live mutation.",
				"evidence": ["The reviewer did not provide enough source evidence."],
				"resolver": "agent",
				"next_action": "Route to needs_evidence with source proof instead of invalidating it."
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("cannot route high-severity or high-risk")
	));
}

#[test]
fn review_checkpoint_rejects_current_blocker_without_accepted_binding() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh reviewer tried to create an unbound current blocker route"],
			"finding_routes": [{
				"route": "current_blocker",
				"severity": "medium",
				"risk_tier": "medium",
				"summary": "Unbound current blocker route.",
				"evidence": ["The route has no accepted finding binding."],
				"resolver": "agent",
				"next_action": "Bind current blockers to accepted findings before repair."
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("`finding_routes.route` `current_blocker` must bind to an `accepted_findings` item")
	));
}

#[test]
fn review_checkpoint_rejects_out_of_range_accepted_route_binding() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh reviewer tried to bind a route to a missing accepted finding"],
			"accepted_findings": accepted_review_findings_json(),
			"finding_routes": [{
				"route": "current_blocker",
				"severity": "medium",
				"risk_tier": "medium",
				"summary": "Out-of-range accepted finding binding.",
				"evidence": ["Only one accepted finding exists."],
				"resolver": "agent",
				"next_action": "Bind to an existing accepted finding index.",
				"finding_source": "accepted_findings",
				"finding_index": 99
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("`finding_routes.finding_index` `99` does not match any accepted finding")
	));
}

#[test]
fn review_checkpoint_rejects_out_of_range_rejected_route_binding() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh reviewer tried to bind a route to a missing rejected finding"],
			"rejected_findings": [{
				"severity": "low",
				"summary": "Reviewer requested unrelated follow-up work.",
				"rejection_reason": "The request is outside the current issue contract.",
				"evidence": ["The current diff does not touch that surface."]
			}],
			"finding_routes": [{
				"route": "reviewer_rubric_gap",
				"severity": "low",
				"risk_tier": "low",
				"summary": "Out-of-range rejected finding binding.",
				"evidence": ["Only one rejected finding exists."],
				"resolver": "reviewer",
				"next_action": "Bind to an existing rejected finding index.",
				"finding_source": "rejected_findings",
				"finding_index": 99
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("`finding_routes.finding_index` `99` does not match any rejected finding")
	));
}

#[test]
fn review_checkpoint_rejects_bound_high_severity_invalid_route() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh reviewer disputed a severe accepted finding"],
			"accepted_findings": [{
				"severity": "high",
				"summary": "Accepted reviewer finding reports a high severity regression.",
				"evidence": ["The reviewer evidence points at the current lane head."],
				"file": "apps/decodex/src/agent/tracker_tool_bridge/tools.rs",
				"line": 1,
				"guidance": "Repair the accepted high severity regression."
			}],
			"finding_routes": [{
				"route": "invalid_or_unsubstantiated",
				"severity": "low",
				"risk_tier": "low",
				"summary": "Route tries to downgrade the accepted finding.",
				"evidence": ["The bound accepted finding is high severity."],
				"resolver": "agent",
				"next_action": "Route to needs_evidence or a landing blocker instead.",
				"finding_source": "accepted_findings",
				"finding_index": 0
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("cannot route high-severity or high-risk")
	));
}

#[test]
fn blocked_review_checkpoint_requires_landing_blocking_route() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "blocked",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["review cannot continue without external evidence"]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires at least one landing-blocking")
	));
}

#[test]
fn review_checkpoint_findings_continue_until_budget_then_stop() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	for expected_round in [1_i64, 2_i64] {
		let response = submit_findings_review_checkpoint(&bridge, "owned fix still pending");

		assert!(response.success);
		assert_eq!(
			DynamicToolHandler::classify_turn_completion(&bridge, "continue")
				.expect("current_blocker repeats below the convergence budget should continue"),
			TurnCompletionStatus::Continue
		);

		let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

		assert_eq!(checkpoint.phase(), "handoff");
		assert_eq!(checkpoint.status(), "findings");
		assert_eq!(checkpoint.nonclean_rounds(), expected_round);
	}

	let response = submit_findings_review_checkpoint(&bridge, "still not converged");

	assert!(!response.success);
	assert!(
		matches!(
			response.content_items.first(),
			Some(DynamicToolContentItem::InputText { text })
				if text.contains("Review churn threshold exceeded")
		),
		"third current_blocker repeat checkpoint should fail immediately: {response:?}"
	);

	let error = DynamicToolHandler::classify_turn_completion(&bridge, "stop")
		.expect_err("third current_blocker repeat checkpoint should stop the lane");
	let stop = error
		.downcast_ref::<ReviewPolicyStopRequested>()
		.expect("stop boundary should expose a typed review policy error");

	assert_eq!(stop.reason, ReviewPolicyStopReason::Exhausted);
	assert_eq!(stop.nonclean_rounds, Some(3));
	assert!(
		stop.fingerprint.as_deref().is_some_and(|fingerprint| {
			fingerprint.starts_with("review_finding:")
		}),
		"stop should identify the repeated finding fingerprint: {stop:?}"
	);

	let fourth_response =
		submit_findings_review_checkpoint(&bridge, "attempted fourth findings checkpoint");

	assert!(!fourth_response.success);
	assert!(
		matches!(
			fourth_response.content_items.first(),
			Some(DynamicToolContentItem::InputText { text })
				if text.contains("Review churn threshold already exceeded")
		),
		"fourth consecutive findings checkpoint should be rejected before persistence: {fourth_response:?}"
	);

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.nonclean_rounds(), 3);

	let fenced_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "implementing",
			"focus": "Continue repairing after review findings.",
			"next_action": "Keep editing the same repair strategy.",
			"blockers": [],
			"evidence": ["The review checkpoint already exceeded the convergence budget."],
			"verification": [],
			"head_sha": sample_local_repo().head_oid,
			"branch": "x/decodex-1"
		}),
	);

	assert!(!fenced_response.success);
	assert!(
		matches!(
			fenced_response.content_items.first(),
			Some(DynamicToolContentItem::InputText { text })
				if text.contains("Review policy stop `review_policy_exhausted` is active")
					&& text.contains("issue_progress_checkpoint")
		),
		"review policy stop should fence mutable progress writes: {fenced_response:?}"
	);
	assert!(
		tracker.comments.borrow().is_empty(),
		"fenced progress checkpoint must not write a tracker comment"
	);
}

#[test]
fn review_checkpoint_distinct_findings_do_not_inherit_old_churn() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	for _round in 1..=2 {
		let response = submit_findings_review_checkpoint(&bridge, "same finding still pending");

		assert!(response.success);
	}

	let distinct_findings = accepted_review_findings_with_summary_json(
		"Distinct reviewer finding",
		"Repair the separate accepted issue before requesting another checkpoint.",
		12,
	);
	let response = submit_findings_review_checkpoint_with_findings(
		&bridge,
		"new finding discovered after the earlier one was repaired",
		distinct_findings,
	);

	assert!(response.success, "new fingerprints should not trip old churn: {response:?}");

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");
	let finding_policy = &details["finding_policy"];
	let records = finding_policy["findings"].as_array().expect("finding records should persist");

	assert_eq!(checkpoint.nonclean_rounds(), 1);
	assert_eq!(finding_policy["active_fingerprints"].as_array().expect("active fingerprints").len(), 1);
	assert!(records.iter().any(|record| {
		record["title"] == "Accepted reviewer finding"
			&& record["status"] == "resolved"
			&& record["repeat_count"] == 2
	}));
	assert!(records.iter().any(|record| {
		record["title"] == "Distinct reviewer finding"
			&& record["status"] == "open"
			&& record["repeat_count"] == 1
	}));
}

#[test]
fn review_checkpoint_clean_resets_nonclean_rounds_before_next_findings() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
		Ok(sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	for status in ["findings", "findings", "clean", "findings"] {
		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": status,
				"head_sha": sample_local_repo().head_oid,
				"review_contract": handoff_review_contract_json(),
				"checks": review_checks_json(),
				"evidence": ["review evidence"],
				"accepted_findings": accepted_review_findings_for_status_json(status)
			}),
		);

		assert!(response.success);
	}

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.nonclean_rounds(), 1);
	assert_eq!(
		DynamicToolHandler::classify_turn_completion(&bridge, "continue")
			.expect("findings after a clean checkpoint should continue"),
		TurnCompletionStatus::Continue
	);
}

#[test]
fn review_checkpoint_does_not_depend_on_tracker_comment_write() {
	let tracker = FakeTracker::with_comment_error("tracker write failed");
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(sample_local_repo()), Ok(sample_local_repo())]);
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
			"status": "findings",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["tracker write failed before checkpoint persisted"],
			"accepted_findings": accepted_review_findings_json()
		}),
	);

	assert!(response.success);
	assert!(tracker.comments.borrow().is_empty());

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

	assert_eq!(checkpoint.nonclean_rounds(), 1);
}

#[test]
fn review_checkpoint_architecture_and_blocked_statuses_stop_immediately() {
	for (status, expected_reason) in [
		("needs_architecture_review", ReviewPolicyStopReason::ArchitectureReviewRequired),
		("blocked", ReviewPolicyStopReason::Blocked),
	] {
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
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": status,
			"head_sha": sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["requires human follow-up"],
			"finding_routes": route_only_review_route_json(if status == "blocked" {
				"landing_blocker"
			} else {
				"architecture_signal"
			})
		}),
	);

		assert!(response.success);

		let error = DynamicToolHandler::classify_turn_completion(&bridge, "stop")
			.expect_err("stop statuses should fail immediately");
		let stop = error
			.downcast_ref::<ReviewPolicyStopRequested>()
			.expect("stop boundary should expose a typed review policy error");

		assert_eq!(stop.reason, expected_reason);
	}
}

#[test]
fn review_checkpoint_phase_switch_resets_nonclean_rounds() {
	let tracker = FakeTracker::new();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let repair_context = sample_review_repair_context_in(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/242",
		);
	let issue = sample_review_issue();
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		repair_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	write_review_policy_checkpoint(
		&bridge,
		&issue,
		&ReviewHandoffContext { mode: ReviewExecutionMode::Handoff, ..repair_context.clone() },
		"handoff",
		"findings",
		&sample_local_repo().head_oid,
		2,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": sample_local_repo().head_oid,
			"review_contract": repair_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh repair-phase review found accepted work"],
			"accepted_findings": accepted_review_findings_json()
		}),
	);

	assert!(response.success);

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &repair_context);

	assert_eq!(checkpoint.phase(), "repair");
	assert_eq!(checkpoint.nonclean_rounds(), 1);
}

#[test]
fn repair_review_checkpoint_stores_accepted_findings_for_repair_loop() {
	let tracker = FakeTracker::new();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let repair_context = sample_review_repair_context_in(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/242",
	);
	let issue = sample_review_issue();
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
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
			"head_sha": sample_local_repo().head_oid,
			"review_contract": repair_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": ["fresh-context retained repair review accepted one finding"],
			"accepted_findings": accepted_review_findings_json(),
			"rejected_findings": [{
				"severity": "info",
				"summary": "Reviewer suggested changing unrelated landing code.",
				"rejection_reason": "Outside this retained repair batch.",
				"evidence": ["The current PR feedback only concerns the tracker-tool bridge."]
			}]
		}),
	);

	assert!(response.success);

	let checkpoint = persisted_review_policy_checkpoint(&bridge, &issue, &repair_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.phase(), "repair");
	assert_eq!(details["review_contract"]["review_type"], "repair_verification");
	assert_eq!(details["accepted_findings"][0]["summary"], "Accepted reviewer finding");
	assert_eq!(details["rejected_findings"][0]["rejection_reason"], "Outside this retained repair batch.");
}

#[test]
fn stale_review_checkpoint_for_old_head_does_not_stop_new_head() {
	let tracker = FakeTracker::new();
	let issue = sample_issue();
	let workflow = sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = sample_review_context_in(temp_dir.path());
	let mut updated_local_repo = sample_local_repo();

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

	write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"blocked",
		&sample_local_repo().head_oid,
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
	let issue = sample_issue();
	let workflow = sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(sample_pull_request())]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		sample_review_context_in(temp_dir.path()),
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
		let workflow = sample_workflow();

		if completion_path == "handoff" {
			let issue = sample_issue();
			let inspector = FakePullRequestInspector::new(vec![Ok(sample_pull_request())]);
			let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(sample_local_repo())]);
			let mut review_context = sample_review_context_in(temp_dir.path());

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
			let review_issue = sample_review_issue();
			let pr_url = "https://github.com/hack-ink/decodex/pull/242";
			let (repair_inspector, repair_local_repo_inspector) =
				sample_review_repair_apply_inspectors(pr_url);
			let mut review_context = sample_review_repair_context_in(temp_dir.path(), pr_url);

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
	let issue = sample_issue();
	let workflow = sample_workflow();
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let mut review_context = sample_review_context_in(temp_dir.path());

	review_context.review_level = ReviewLevel::Off;

	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"findings",
		&sample_local_repo().head_oid,
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
	let issue = sample_issue();
	let workflow = sample_workflow();
	let mut updated_local_repo = sample_local_repo();
	let mut updated_pull_request = sample_pull_request();

	updated_local_repo.head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
	updated_pull_request.head_ref_oid = updated_local_repo.head_oid.clone();
	updated_pull_request.url = String::from("https://github.com/hack-ink/decodex/pull/149");

	let review_context = sample_review_context_in(temp_dir.path());
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

	write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"clean",
		&sample_local_repo().head_oid,
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
	let issue = sample_issue();
	let workflow = sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(sample_pull_request())]);
	let review_context = sample_review_context_in(temp_dir.path());
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(sample_dirty_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"clean",
		&sample_local_repo().head_oid,
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
	let issue = sample_review_issue();
	let workflow = sample_workflow();
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
	let review_context = sample_review_repair_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	bridge_state_store(&bridge)
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
	let mut issue = sample_review_issue();

	issue
		.team
		.states
		.push(TrackerState { id: String::from("state-done"), name: String::from("Done") });

	let tracker = tracker_with_current_issue_snapshot(&issue);
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
max_concurrent_agents = 1
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
		sample_closeout_context_in(temp_dir.path(), pr_url),
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

#[test]
fn review_repair_apply_persists_updated_handoff_marker_without_tracker_transition() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = sample_review_issue();
	let workflow = sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let (inspector, local_repo_inspector) = sample_review_repair_apply_inspectors(pr_url);
	let review_context = sample_review_repair_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	seed_review_repair_apply_state(
		bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		pr_url,
		2,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Addressed the requested review changes."
		}),
	);

	seed_docs_impact_checkpoint(
		bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"review_repair",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_repair" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	assert_review_policy_checkpoint_cleared(&bridge, &issue, &review_context);

	DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect("review repair completion should allow the turn to complete");

	bridge.apply_review_repair().expect("review repair should apply");

	assert!(tracker.state_updates.borrow().is_empty());

	let comments = tracker.comments.borrow();

	assert_eq!(comments.len(), 1);
	assert!(comments[0].contains("fresh review"));
	assert!(comments[0].contains("- pr_url: `https://github.com/hack-ink/decodex/pull/242`"));

	let marker = persisted_review_handoff_marker(&bridge, &issue, &review_context);

	assert_eq!(marker.pr_url(), pr_url);
	assert_eq!(marker.pr_head_oid(), "18a20f7dfb9526e7421a5f095b1c6adec84e52d6");

	let orchestration_marker =
		persisted_review_orchestration_marker(&bridge, &issue, &review_context, &marker);

	assert_eq!(orchestration_marker.phase(), "request_pending");
	assert_eq!(orchestration_marker.head_sha(), "18a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(orchestration_marker.external_round_count(), 2);
}

#[test]
fn review_repair_apply_does_not_reset_external_round_budget_after_fourth_round() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = sample_review_issue();
	let workflow = sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let (inspector, local_repo_inspector) = sample_review_repair_apply_inspectors(pr_url);
	let review_context = sample_review_repair_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	seed_review_repair_apply_state(
		bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		pr_url,
		4,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Addressed the requested review changes."
		}),
	);

	seed_docs_impact_checkpoint(
		bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"review_repair",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_repair" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect("review repair completion should allow the turn to complete");

	bridge.apply_review_repair().expect("review repair should apply");

	let marker = persisted_review_handoff_marker(&bridge, &issue, &review_context);
	let orchestration_marker =
		persisted_review_orchestration_marker(&bridge, &issue, &review_context, &marker);

	assert_eq!(orchestration_marker.phase(), "request_pending");
	assert_eq!(orchestration_marker.external_round_count(), 4);
}

#[test]
fn review_repair_apply_preserves_existing_markers_when_comment_write_fails() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::with_comment_error("tracker comment write failed");
	let issue = sample_review_issue();
	let workflow = sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let (inspector, local_repo_inspector) = sample_review_repair_apply_inspectors(pr_url);
	let review_context = sample_review_repair_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);
	let seed_context = sample_review_repair_context_in(temp_dir.path(), pr_url);

	seed_review_repair_apply_state(
		bridge_state_store(&bridge),
		&seed_context,
		&issue.id,
		pr_url,
		2,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Addressed the requested review changes."
		}),
	);

	seed_docs_impact_checkpoint(
		bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"review_repair",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_repair" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	let error = bridge
		.apply_review_repair()
		.expect_err("comment write failures must preserve the original handoff marker");

	assert!(error.to_string().contains("tracker comment write failed"));
	assert!(tracker.comments.borrow().is_empty());

	let marker = persisted_review_handoff_marker(&bridge, &issue, &seed_context);

	assert_eq!(marker.pr_url(), pr_url);
	assert_eq!(marker.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");

	let orchestration_marker =
		persisted_review_orchestration_marker(&bridge, &issue, &seed_context, &marker);

	assert_eq!(orchestration_marker.phase(), "repair_required");
	assert_eq!(orchestration_marker.head_sha(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(orchestration_marker.external_round_count(), 2);
}
