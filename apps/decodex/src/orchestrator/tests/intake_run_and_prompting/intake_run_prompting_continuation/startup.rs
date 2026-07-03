use crate::{
	agent::{DynamicToolHandler, TrackerToolBridge},
	orchestrator::{
		ISSUE_TRANSITION_TOOL_NAME, IssueDispatchMode, IssueTurnContinuationGuard,
		TurnContinuationGuard,
		tests::{self, FakeTracker, TEST_SERVICE_ID, intake_run_and_prompting},
	},
};

#[test]
fn continuation_guard_rejects_first_turn_without_startup_transition() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
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
		dispatch_mode: IssueDispatchMode::Normal,
		review_state_inspector: None,
	};
	let error = guard
		.validate_continuation_boundary(1)
		.expect_err("turn 1 should fail if the startup transition never happened");

	assert!(error.to_string().contains("ended without moving the tracker issue to `In Progress`"));
}

#[test]
fn continuation_guard_allows_local_startup_transition_on_stale_rereads() {
	{
		let (_temp_dir, _config, workflow) = tests::temp_project_layout();
		let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("Todo");
		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
		let transition_response = DynamicToolHandler::handle_call(
			&tracker_tool_bridge,
			ISSUE_TRANSITION_TOOL_NAME,
			serde_json::json!({ "state": "In Progress" }),
		);

		assert!(transition_response.success);

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
			dispatch_mode: IssueDispatchMode::Normal,
			review_state_inspector: None,
		};

		assert!(
			guard
				.should_continue_turn(1)
				.expect("a stale pre-write reread should not block turn-one continuation")
		);

		guard.validate_continuation_boundary(1).expect(
			"a stale pre-write reread should not hard-fail turn one after a local startup transition",
		);
	}
	{
		let (_temp_dir, _config, workflow) = tests::temp_project_layout();
		let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("Todo");
		let tracker = FakeTracker::with_refresh_snapshots(
			vec![issue.clone()],
			vec![vec![issue.clone()], vec![issue.clone()]],
		);
		let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
		let transition_response = DynamicToolHandler::handle_call(
			&tracker_tool_bridge,
			ISSUE_TRANSITION_TOOL_NAME,
			serde_json::json!({ "state": "In Progress" }),
		);

		assert!(transition_response.success);

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
			dispatch_mode: IssueDispatchMode::Normal,
			review_state_inspector: None,
		};

		assert!(
			guard
				.should_continue_turn(1)
				.expect("a stale pre-write reread should not block turn-one continuation")
		);
		assert!(
			guard
				.should_continue_turn(2)
				.expect("a stale pre-write reread should remain tolerated after turn one")
		);
	}
}
