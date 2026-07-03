use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, TrackerToolBridge,
		TurnCompletionStatus,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	tracker::{TrackerLabel, TrackerState},
};

#[test]
fn turn_completion_rejects_xy_156_shape_without_terminal_tracker_action() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context(),
		&inspector,
		&local_repo_inspector,
	);
	let error = DynamicToolHandler::validate_turn_completion(
		&bridge,
		"Implementation and tests are done, but commit, push, PR, and tracker handoff remain.",
	)
	.expect_err("turn completion should reject missing terminal tracker actions");

	assert!(error.to_string().contains("recorded neither `issue_review_handoff`"));
}

#[test]
fn turn_classification_allows_continuation_without_terminal_tracker_action() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context(),
		&inspector,
		&local_repo_inspector,
	);

	assert_eq!(
		DynamicToolHandler::classify_turn_completion(
			&bridge,
			"Still implementing; no terminal tracker action has been recorded yet."
		)
		.expect("missing terminal action should request continuation"),
		TurnCompletionStatus::Continue
	);
}

#[test]
fn turn_classification_rejects_clean_closeout_continuation() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let merged_pull_request = {
		let mut pull_request = tests::sample_pull_request();

		pull_request.url = String::from(pr_url);
		pull_request.state = String::from("MERGED");

		pull_request
	};
	let inspector = FakePullRequestInspector::new(vec![Ok(merged_pull_request)]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		&inspector,
		&local_repo_inspector,
	);
	let error = DynamicToolHandler::classify_turn_completion(
		&bridge,
		"Still re-reading merged closeout context; no terminal tracker action has been recorded yet.",
	)
	.expect_err("closeout should not yield another clean continuation boundary");

	assert!(error.to_string().contains("deterministic tail"));
	assert!(error.to_string().contains(ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME));
	assert!(error.to_string().contains(ISSUE_TERMINAL_FINALIZE_TOOL_NAME));
}

#[test]
fn turn_classification_rejects_continuation_blocking_writes_without_terminal_path() {
	for (tool_name, args) in [
		(ISSUE_LABEL_ADD_TOOL_NAME, serde_json::json!({ "label": "decodex:manual-only" })),
		(ISSUE_TRANSITION_TOOL_NAME, serde_json::json!({ "state": "Todo" })),
	] {
		let mut refreshed_issue = tests::sample_issue();

		if tool_name == ISSUE_LABEL_ADD_TOOL_NAME {
			refreshed_issue.labels.push(TrackerLabel {
				id: String::from("label-manual"),
				name: String::from("decodex:manual-only"),
			});
		} else {
			refreshed_issue.state =
				TrackerState { id: String::from("state-todo"), name: String::from("Todo") };
		}

		let tracker = FakeTracker::with_refresh_snapshots(vec![vec![refreshed_issue]]);
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let inspector = FakePullRequestInspector::new(Vec::new());
		let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			tests::sample_review_context(),
			&inspector,
			&local_repo_inspector,
		);
		let response = DynamicToolHandler::handle_call(&bridge, tool_name, args);

		assert!(response.success);

		let error = DynamicToolHandler::classify_turn_completion(
			&bridge,
			"The lane recorded a continuation-blocking tracker write without a terminal path.",
		)
		.expect_err("continuation-blocking writes must not exit via a clean boundary");

		assert!(error.to_string().contains("without recording a terminal path"));
		assert!(error.to_string().contains(tool_name));
	}
}

#[test]
fn turn_classification_rejects_continuation_blocking_writes_for_stale_active_refresh() {
	for (tool_name, args) in [
		(ISSUE_LABEL_ADD_TOOL_NAME, serde_json::json!({ "label": "decodex:manual-only" })),
		(ISSUE_TRANSITION_TOOL_NAME, serde_json::json!({ "state": "Todo" })),
	] {
		let active_issue = tests::sample_in_progress_issue();
		let tracker = FakeTracker::with_refresh_snapshots(vec![vec![active_issue]]);
		let issue = tests::sample_in_progress_issue();
		let workflow = tests::sample_workflow();
		let inspector = FakePullRequestInspector::new(Vec::new());
		let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			tests::sample_review_context(),
			&inspector,
			&local_repo_inspector,
		);
		let response = DynamicToolHandler::handle_call(&bridge, tool_name, args);

		assert!(response.success);

		let error = DynamicToolHandler::classify_turn_completion(
			&bridge,
			"The run started active, so a stale active reread must not clear a local stop write.",
		)
		.expect_err("active-start lanes must keep local stop writes blocking");

		assert!(error.to_string().contains("without recording a terminal path"));
		assert!(error.to_string().contains(tool_name));
	}
}

#[test]
fn turn_classification_allows_continuation_blocking_writes_after_reactivation() {
	for (tool_name, args) in [
		(ISSUE_LABEL_ADD_TOOL_NAME, serde_json::json!({ "label": "decodex:manual-only" })),
		(ISSUE_TRANSITION_TOOL_NAME, serde_json::json!({ "state": "Todo" })),
	] {
		let mut reactivated_issue = tests::sample_issue();

		reactivated_issue.state =
			TrackerState { id: String::from("state-progress"), name: String::from("In Progress") };

		let tracker = FakeTracker::with_refresh_snapshots(vec![
			vec![reactivated_issue.clone()],
			vec![reactivated_issue],
		]);
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let inspector = FakePullRequestInspector::new(Vec::new());
		let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			tests::sample_review_context(),
			&inspector,
			&local_repo_inspector,
		);
		let response = DynamicToolHandler::handle_call(&bridge, tool_name, args);

		assert!(response.success);
		assert_eq!(
			DynamicToolHandler::classify_turn_completion(
				&bridge,
				"The issue was reactivated before turn completion, so the stale stop write must not block continuation."
			)
			.expect("startable-start lanes should allow continuation after reactivation"),
			TurnCompletionStatus::Continue
		);
	}
}

#[test]
fn turn_classification_rejects_continuation_blocking_write_when_refresh_returns_no_snapshot() {
	let mut opted_out_issue = tests::sample_issue();

	opted_out_issue.labels.push(TrackerLabel {
		id: String::from("label-manual"),
		name: String::from("decodex:manual-only"),
	});

	let tracker = FakeTracker::with_refresh_snapshots(vec![vec![opted_out_issue], Vec::new()]);
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context(),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:manual-only" }),
	);

	assert!(response.success);

	let error = DynamicToolHandler::classify_turn_completion(
		&bridge,
		"The lane recorded a continuation-blocking tracker write without a terminal path.",
	)
	.expect_err("missing refresh snapshots must not allow a clean continuation boundary");

	assert!(error.to_string().contains("without recording a terminal path"));
	assert!(error.to_string().contains(ISSUE_LABEL_ADD_TOOL_NAME));
}
