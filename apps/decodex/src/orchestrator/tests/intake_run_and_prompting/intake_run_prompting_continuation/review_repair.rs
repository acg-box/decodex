use crate::{
	agent::TrackerToolBridge,
	orchestrator::{
		IssueDispatchMode, IssueTurnContinuationGuard, TurnContinuationGuard,
		tests::{self, FakeTracker, TEST_SERVICE_ID, intake_run_and_prompting},
	},
};

#[test]
fn continuation_guard_allows_review_repair_continuation_while_issue_remains_in_review() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: &issue.state.name,
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		review_state_inspector: None,
	};

	assert!(
		guard
			.should_continue_turn(2)
			.expect("retained review-repair lane should continue while issue remains in review")
	);

	guard.validate_continuation_boundary(2).expect(
		"review-repair continuation boundary should stay valid while the issue remains in review",
	);
}
