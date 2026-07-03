use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
		},
	},
	tracker::{
		self, TrackerComment, TrackerLabel, TrackerState,
		records::{self, CLOSEOUT_RECORD_TYPE, CloseoutRecord},
	},
};

#[test]
fn closeout_apply_validates_merged_pr_and_completed_issue_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	let tracker = FakeTracker::with_refresh_snapshots(vec![
		vec![completed_issue.clone()],
		vec![completed_issue],
	]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_closeout_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
		"pr_url": pr_url,
		"summary": "Merged the approved lane and finished closeout."
		}),
	);

	tests::seed_docs_impact_checkpoint(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"closeout",
		&tests::sample_local_repo().head_oid,
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "closeout" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect("closeout completion should allow the turn to complete");

	bridge.apply_closeout().expect("closeout should validate cleanly");

	let comments = tracker.comments.borrow();

	assert_eq!(comments.len(), 1);
	assert!(comments[0].contains("decodex closeout completed"));
	assert!(comments[0].contains("\"record_type\": \"decodex.linear_execution_event\""));
	assert!(comments[0].contains("\"event_type\": \"closeout\""));
	assert!(tracker.state_updates.borrow().is_empty());
}

#[test]
fn closeout_apply_writes_coarse_comment_without_replaying_existing_records() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	completed_issue.labels.push(TrackerLabel {
		id: String::from("label-active"),
		name: tracker::automation_active_label(TEST_SERVICE_ID),
	});

	let tracker = FakeTracker::with_refresh_snapshots(vec![
		vec![completed_issue.clone()],
		vec![completed_issue],
	]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let existing_record = records::append_structured_comment_record(
		"decodex closeout completed",
		&CloseoutRecord {
			record_type: String::from(CLOSEOUT_RECORD_TYPE),
			completed_at: String::from("2026-04-12T00:00:00Z"),
			run_id: String::from("pub-618-attempt-4-123"),
			attempt_number: 4,
			branch_name: String::from("x/decodex-pub-618"),
			pr_url: String::from(pr_url),
		},
	)
	.expect("closeout record should serialize");

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![TrackerComment {
			body: existing_record,
			created_at: String::from("2026-04-12T00:00:00Z"),
		}],
	);

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_closeout_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
		"pr_url": pr_url,
		"summary": "Merged the approved lane and finished closeout."
		}),
	);

	tests::seed_docs_impact_checkpoint(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"closeout",
		&tests::sample_local_repo().head_oid,
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "closeout" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	bridge.apply_closeout().expect("closeout should persist a coarse tracker summary");

	assert_eq!(tracker.comments.borrow().len(), 1);
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"closeout writeback should keep ownership until cleanup succeeds"
	);
	assert!(tracker.state_updates.borrow().is_empty());
}

#[test]
fn closeout_clear_uses_server_team_label_lookup_for_active_label_removal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	completed_issue
		.labels
		.push(TrackerLabel { id: String::from("label-active"), name: active_label.clone() });
	completed_issue
		.labels
		.push(TrackerLabel { id: String::from("label-queued"), name: queue_label.clone() });
	completed_issue.team.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::with_refresh_snapshots(vec![
		vec![completed_issue.clone()],
		vec![completed_issue.clone()],
	])
	.with_team_label_lookup_id(&completed_issue.team.id, &active_label, "label-active");
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_closeout_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
		"pr_url": pr_url,
		"summary": "Merged the approved lane and finished closeout."
		}),
	);

	tests::seed_docs_impact_checkpoint(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"closeout",
		&tests::sample_local_repo().head_oid,
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "closeout" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	bridge.clear_closeout_issue_scope().expect(
		"closeout cleanup should resolve the active label id server-side when team labels paginate",
	);

	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[vec![String::from("label-active")], vec![String::from("label-queued")],]
	);
	assert!(tracker.state_updates.borrow().is_empty());
}

#[test]
fn closeout_apply_keeps_active_label_until_cleanup() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };
	completed_issue.labels_complete = false;

	completed_issue.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::with_refresh_snapshots(vec![
		vec![completed_issue.clone()],
		vec![completed_issue.clone()],
	])
	.with_label_lookup_issues(&active_label, vec![completed_issue.clone()]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_closeout_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
		"pr_url": pr_url,
		"summary": "Merged the approved lane and finished closeout."
		}),
	);

	tests::seed_docs_impact_checkpoint(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"closeout",
		&tests::sample_local_repo().head_oid,
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "closeout" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	bridge
		.apply_closeout()
		.expect("closeout writeback should succeed without clearing the active label");

	assert!(
		tracker.label_removals.borrow().is_empty(),
		"closeout writeback should not clear ownership before cleanup"
	);
	assert!(tracker.label_updates.borrow().is_empty());
}

#[test]
fn closeout_clear_clears_active_label_when_issue_labels_paginate() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };
	completed_issue.labels_complete = false;

	completed_issue.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::with_refresh_snapshots(vec![vec![completed_issue.clone()]])
		.with_label_lookup_issues(&active_label, vec![completed_issue.clone()])
		.with_label_lookup_issues(&queue_label, vec![completed_issue.clone()]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let merged_pull_request = {
		let mut pull_request = tests::sample_pull_request();

		pull_request.url = String::from(pr_url);
		pull_request.state = String::from("MERGED");

		pull_request
	};
	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);

	bridge
		.clear_closeout_issue_scope()
		.expect("closeout cleanup should clear the active and queue labels incrementally when issue labels paginate");

	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[vec![String::from("label-active")], vec![String::from("label-queued")],]
	);
}

#[test]
fn closeout_clear_treats_missing_lane_label_removal_as_idempotent() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	completed_issue
		.labels
		.push(TrackerLabel { id: String::from("label-active"), name: active_label });
	completed_issue
		.labels
		.push(TrackerLabel { id: String::from("label-queued"), name: queue_label });

	let tracker =
		FakeTracker::with_label_update_error("Linear GraphQL request failed: Label not on issue");

	tracker.refresh_snapshots.replace(vec![vec![completed_issue.clone()]]);

	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let merged_pull_request = {
		let mut pull_request = tests::sample_pull_request();

		pull_request.url = String::from(pr_url);
		pull_request.state = String::from("MERGED");

		pull_request
	};
	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);

	bridge
		.clear_closeout_issue_scope()
		.expect("closeout cleanup should ignore already-absent Linear lane labels");
}

#[test]
fn closeout_clear_skips_lane_labels_when_server_confirms_absent() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	completed_issue
		.labels
		.retain(|label| label.name != active_label.as_str() && label.name != queue_label.as_str());

	let tracker = FakeTracker::with_refresh_snapshots(vec![vec![completed_issue.clone()]]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let merged_pull_request = {
		let mut pull_request = tests::sample_pull_request();

		pull_request.url = String::from(pr_url);
		pull_request.state = String::from("MERGED");

		pull_request
	};
	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);

	bridge
		.clear_closeout_issue_scope()
		.expect("closeout cleanup should be idempotent after lane labels are already gone");

	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn closeout_complete_rejects_issue_that_is_not_yet_completed() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = tests::tracker_with_current_issue_snapshot(&tests::sample_review_issue());
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/261";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Merged the approved lane and attempted closeout."
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires tracker state `Done`")
				&& text.contains("Move the issue to `Done` with `issue_transition` before calling `issue_closeout_complete`")
	));
}
