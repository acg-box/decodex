use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::tests::{
	self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
};
use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, PullRequestDetails, RunCompletionDisposition,
		TrackerToolBridge, TurnCompletionStatus,
	},
	tracker::{TrackerLabel, TrackerState, records},
};

#[test]
fn completion_disposition_allows_manual_attention_exit_without_review_handoff() {
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
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
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let comment_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(response.success);
	assert!(comment_response.success);

	let comment =
		tracker.comments.borrow().first().expect("manual attention comment should write").clone();
	let record = records::parse_linear_execution_event_record(&comment)
		.expect("manual attention comment should include a ledger record");

	assert_eq!(record.event_type, "needs_attention");
	assert_eq!(record.error_class.as_deref(), Some("operator_decision_required"));
	assert_eq!(record.terminal_path.as_deref(), Some("manual_attention"));
	assert_eq!(
		bridge.completion_disposition().expect("manual attention should be accepted"),
		RunCompletionDisposition::ManualAttention
	);
}

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
fn manual_attention_requires_explanatory_comment() {
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
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
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);

	assert!(response.success);
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"manual-attention intent alone must not mutate Linear"
	);

	let error = bridge
		.completion_disposition()
		.expect_err("manual attention must require an explanatory comment");

	assert!(error.to_string().contains("never recorded the required explanatory comment"));
}

#[test]
fn failed_needs_attention_label_update_does_not_record_manual_attention() {
	let tracker = FakeTracker::with_label_update_error("tracker labels unavailable");
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
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let comment_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(response.success);
	assert!(!comment_response.success);
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().is_empty());

	let error = bridge
		.completion_disposition()
		.expect_err("failed label writes must not count as manual attention");

	assert!(error.to_string().contains("never recorded the required explanatory comment"));
}

#[test]
fn opt_out_label_add_uses_refreshed_issue_snapshot_for_label_ids() {
	let initial_issue = tests::sample_issue();
	let mut refreshed_issue = initial_issue.clone();

	refreshed_issue.labels.push(TrackerLabel {
		id: String::from("label-needs"),
		name: String::from("decodex:needs-attention"),
	});

	let tracker = FakeTracker::with_refresh_snapshots(vec![vec![refreshed_issue]]);
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &initial_issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:manual-only" }),
	);

	assert!(response.success);
	assert_eq!(tracker.label_additions.borrow().as_slice(), [vec![String::from("label-manual")]]);
}

#[test]
fn label_add_fails_when_refresh_returns_no_snapshot() {
	let tracker = FakeTracker::with_refresh_snapshots(vec![Vec::new()]);
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:manual-only" }),
	);

	assert!(!response.success);
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: format!(
				"Failed to refresh issue `{}` before updating labels: tracker returned no current snapshot.",
				issue.identifier
			),
		}]
	);
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
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

#[test]
fn completion_disposition_rejects_conflicting_review_handoff_and_manual_attention() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/48"),
	})]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
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

	let review_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/48",
			"summary": "Ready for review."
		}),
	);
	let label_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);

	assert!(review_response.success);
	assert!(label_response.success);

	let error = bridge
		.completion_disposition()
		.expect_err("conflicting completion signals should be rejected");

	assert!(error.to_string().contains("Use exactly one final tracker exit path."));
}
