use crate::{
	agent::TrackerToolBridge,
	orchestrator::{
		IssueDispatchMode, IssueTurnContinuationGuard, TurnContinuationGuard,
		tests::{self, FakeTracker, TEST_SERVICE_ID, intake_run_and_prompting},
	},
};

#[test]
fn continuation_guard_preserves_original_startable_state_across_continuation_retries() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("In Progress");
	let stale_issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("Todo");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![stale_issue.clone()]]);
	let tracker_tool_bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let guard = IssueTurnContinuationGuard {
		tracker: &tracker,
		tracker_tool_bridge: &tracker_tool_bridge,
		workflow: &workflow,
		service_id: TEST_SERVICE_ID,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		initial_issue_state: "Todo",
		retry_project_slug: issue
			.project_slug
			.as_deref()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		review_state_inspector: None,
	};

	assert!(
		guard
			.should_continue_turn(2)
			.expect("continuation retries must preserve the original startable state even after a refreshed in-progress run plan")
	);
}

#[test]
fn continuation_guard_stops_when_service_active_label_is_removed() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![tests::sample_issue("In Progress", &[])]],
	);
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

	assert!(
		!guard
			.should_continue_turn(2)
			.expect("continuation must stop once service ownership is removed"),
	);
}
