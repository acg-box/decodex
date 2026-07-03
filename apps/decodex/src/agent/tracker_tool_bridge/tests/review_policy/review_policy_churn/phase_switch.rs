use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ReviewExecutionMode, ReviewHandoffContext, TempDir,
	TrackerToolBridge, review_policy,
};

#[test]
fn review_checkpoint_phase_switch_resets_nonclean_rounds() {
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

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&ReviewHandoffContext { mode: ReviewExecutionMode::Handoff, ..repair_context.clone() },
		"handoff",
		"findings",
		&tests::sample_local_repo().head_oid,
		2,
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
			"evidence": ["fresh repair-phase review found accepted work"],
			"accepted_findings": review_policy::accepted_review_findings_json()
		}),
	);

	assert!(response.success);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &repair_context);

	assert_eq!(checkpoint.phase(), "repair");
	assert_eq!(checkpoint.nonclean_rounds(), 1);
}
