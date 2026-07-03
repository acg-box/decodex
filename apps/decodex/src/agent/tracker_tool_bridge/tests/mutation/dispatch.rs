use std::process;

use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::tests::{
	self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	GitHubTokenAssertingPullRequestInspector, TEST_SERVICE_ID, TestEnvVarGuard,
};
use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, TrackerToolBridge,
	},
	orchestrator::{
		self, AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition,
		AuthorityBoundaryPolicyDecision,
	},
	state::StateStore,
	tracker::{
		self, TrackerComment, TrackerIssue, TrackerLabel, TrackerState, public_text,
		records::{self, CLOSEOUT_RECORD_TYPE, CloseoutRecord},
	},
	workflow::WorkflowDocument,
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

#[test]
fn review_handoff_inspection_uses_configured_github_token() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let token_env_var = "DECODEX_TEST_REVIEW_HANDOFF_GITHUB_TOKEN";
	let _env_guard = TestEnvVarGuard::set(token_env_var, "configured-review-token");
	let inspector = GitHubTokenAssertingPullRequestInspector {
		expected_token: String::from("configured-review-token"),
		response: tests::sample_pull_request(),
	};
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let mut review_context = tests::sample_review_context_in(temp_dir.path());

	review_context.github_token_env_var = Some(String::from(token_env_var));

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
			"pr_url": "https://github.com/hack-ink/decodex/pull/48",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);
}

#[test]
fn review_handoff_inspection_rejects_missing_or_blank_github_token() {
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
		let local_repo_inspector =
			FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
		let mut review_context = tests::sample_review_context_in(temp_dir.path());

		review_context.github_token_env_var = None;

		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			review_context.clone(),
			&pull_request_inspector,
			&local_repo_inspector,
		);

		tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			serde_json::json!({
				"pr_url": "https://github.com/hack-ink/decodex/pull/48",
				"summary": "Ready for review."
			}),
		);

		assert!(!response.success);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText {
				text: String::from(
					"`github.token_env_var` must be configured for PR-backed review handoff validation.",
				),
			}]
		);
	}
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
		let local_repo_inspector =
			FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
		let env_var =
			format!("DECODEX_TEST_BLANK_REVIEW_HANDOFF_GITHUB_TOKEN_ENV_{}", process::id());
		let _env_guard = TestEnvVarGuard::set(&env_var, "");
		let mut review_context = tests::sample_review_context_in(temp_dir.path());

		review_context.github_token_env_var = Some(env_var.clone());

		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			review_context.clone(),
			&pull_request_inspector,
			&local_repo_inspector,
		);

		tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			serde_json::json!({
				"pr_url": "https://github.com/hack-ink/decodex/pull/48",
				"summary": "Ready for review."
			}),
		);

		assert!(!response.success);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText {
				text: format!(
					"Environment variable `{env_var}` referenced by `github.token_env_var` must not be blank."
				),
			}]
		);
	}
}

#[test]
fn transitions_current_issue_with_allowed_state() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "issue_identifier": "DEC-1", "state": "In Progress" }),
	);

	assert!(response.success);
	assert_eq!(tracker.state_updates.borrow().as_slice(), ["state-progress"]);
}

#[test]
fn rejects_success_transition_without_review_handoff() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "state": "In Review" }),
	);

	assert!(!response.success);
	assert!(tracker.state_updates.borrow().is_empty());
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: String::from(
				"State `In Review` requires `issue_review_handoff` after the branch is pushed and a reviewable PR exists."
			),
		}]
	);
}

#[test]
fn rejects_invalid_transition_argument_shapes() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let cases = [
		(
			serde_json::json!({ "unexpected_state_field": "In Progress" }),
			"Invalid `issue.transition` arguments: missing field `state`",
		),
		(
			serde_json::json!({
				"state": "In Progress",
				"unexpected_state_field": "In Progress",
			}),
			"Invalid `issue.transition` arguments: unknown field `unexpected_state_field`",
		),
	];

	for (arguments, message) in cases {
		let response =
			DynamicToolHandler::handle_call(&bridge, ISSUE_TRANSITION_TOOL_NAME, arguments);

		assert!(!response.success);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText { text: String::from(message) }]
		);
	}

	assert!(tracker.state_updates.borrow().is_empty());
}

#[test]
fn rejects_success_transition_regardless_of_other_workflow_state_membership() {
	for workflow in [
		tests::sample_workflow_with_startable_states(&["Todo", "In Review"]),
		tests::sample_workflow_with_tracker_states(
			&["Todo"],
			"In Progress",
			"In Review",
			"In Review",
		),
		tests::sample_workflow_with_tracker_states(&["Todo"], "In Review", "In Review", "Todo"),
	] {
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();

		assert_success_transition_requires_review_handoff(workflow, &tracker, &issue);
	}
}

fn assert_success_transition_requires_review_handoff(
	workflow: WorkflowDocument,
	tracker: &FakeTracker,
	issue: &TrackerIssue,
) {
	let bridge = TrackerToolBridge::new(tracker, issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "state": "In Review" }),
	);

	assert!(!response.success);
	assert!(tracker.state_updates.borrow().is_empty());
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: String::from(
				"State `In Review` requires `issue_review_handoff` after the branch is pushed and a reviewable PR exists."
			),
		}]
	);
}

#[test]
fn rejects_tool_calls_for_another_issue() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let mut args = tests::manual_attention_comment_args();

	args["issue_identifier"] = serde_json::json!("DEC-999");

	let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn accepts_manual_attention_public_summary_kind() {
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
	let label_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);

	assert!(label_response.success);
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"manual-attention label intent must not mutate Linear before comment validation"
	);

	let comment_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(comment_response.success);

	let comments = tracker.comments.borrow();
	let comment = comments.first().expect("manual attention summary should write");
	let record = records::parse_linear_execution_event_record(comment)
		.expect("manual attention summary should include a ledger record");

	assert_eq!(tracker.label_additions.borrow().as_slice(), [vec![String::from("label-needs")]]);
	assert_eq!(comments.len(), 1);
	assert!(comment.contains("decodex run needs manual attention"));
	assert!(comment.contains("- worktree_path: `.worktrees/PUB-618`"));
	assert!(comment.contains("- failed_command: cargo make test"));
	assert!(comment.contains("- raw_error: repo gate failed with public test output"));
	assert_eq!(record.event_type, "needs_attention");
	assert_eq!(record.error_class.as_deref(), Some("operator_decision_required"));
	assert_eq!(record.failed_command.as_deref(), Some("cargo make test"));
	assert_eq!(record.raw_error.as_deref(), Some("repo gate failed with public test output"));
}

#[test]
fn accepts_authority_boundary_decision_request_without_public_private_evidence_leakage() {
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = tests::sample_workflow();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let private_diff_evidence = "private diff path /Users/example/decodex/.worktrees/DEC-1";
	let review_context = tests::sample_review_context();
	let boundary_event = orchestrator::record_authority_boundary_check_private_event(
		&state_store,
		AuthorityBoundaryCheckInput {
			project_id: &review_context.service_id,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: &review_context.run_id,
			attempt_number: review_context.attempt_number,
			decision_contract_ids: vec!["contract-dec-1"],
			attempted_recovery_reason: "uncovered_direction",
			changed_surfaces: vec![crate::orchestrator::AuthorityBoundaryChangedSurface {
				surface: crate::orchestrator::AuthorityBoundarySurface::Objective,
				change_summary: "Public CLI behavior would change.",
				policy_decision:
					crate::orchestrator::AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
				legacy_disposition:
					crate::orchestrator::AuthorityBoundaryDisposition::RequiresHuman,
			}],
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_disposition_reason: "Accepted behavior needs explicit authority.",
			improvement_signals: Vec::new(),
		},
	)
	.expect("authority boundary check should persist");
	let bridge = TrackerToolBridge::with_run_context_and_state_store(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&state_store,
	);
	let label_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let mut args = tests::manual_attention_comment_args();

	args["error_class"] = serde_json::json!("contract_boundary_required");
	args["next_action"] = serde_json::json!(
		"accept, reject, or revise decision request `dr-dec-1-2`, then clear needs-attention and requeue through Decodex"
	);
	args["decision_request"] = serde_json::json!({
		"boundary_check_id": boundary_event.record_id(),
		"decision_request_id": "dr-dec-1-2",
		"reason_code": "contract_boundary_required",
		"boundary_type": "accepted_behavior",
		"proposed_change": "Change the public CLI contract for retained lane recovery.",
		"why_exceeds_authority": "The accepted issue did not authorize changing public CLI behavior.",
		"options": [
			{
				"label": "accept",
				"description": "Authorize the CLI contract change and update the Decision Contract."
			},
			{
				"label": "reject",
				"description": "Keep the current contract and stop this recovery path."
			}
		],
		"recommendation": "Revise the Decision Contract before resuming automation.",
		"resume_condition": "Automation may resume after the issue or Decision Contract explicitly authorizes the boundary change.",
		"retained_worktree_evidence": ["retained worktree has tracked changes"],
		"retained_diff_evidence": [private_diff_evidence],
		"recovery_attempt_context": ["authority boundary check required human direction"]
	});

	let comment_response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

	assert!(label_response.success);
	assert!(comment_response.success);

	let comments = tracker.comments.borrow();
	let comment = comments.first().expect("decision request summary should write");
	let record = records::parse_linear_execution_event_record(comment)
		.expect("decision request summary should include a ledger record");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private decision event should list");
	let decision_event = events
		.iter()
		.find(|event| event.event_type() == "authority_decision_request")
		.expect("private decision request should persist");

	assert_eq!(record.event_type, "needs_attention");
	assert_eq!(record.error_class.as_deref(), Some("contract_boundary_required"));
	assert!(comment.contains("- decision_request_id: `dr-dec-1-2`"));
	assert!(comment.contains("- boundary: `accepted_behavior`"));
	assert!(comment.contains("- recommendation: Revise the Decision Contract"));
	assert!(!comment.contains(private_diff_evidence));
	assert_eq!(decision_event.payload()["decision_request_id"], serde_json::json!("dr-dec-1-2"));
	assert_eq!(
		decision_event.payload()["retained_diff_evidence"][0],
		serde_json::json!(private_diff_evidence)
	);
}

#[test]
fn rejects_arbitrary_issue_comment_bodies() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		serde_json::json!({ "body": "Started work and running validation now." }),
	);

	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Invalid `issue.comment` arguments")
	));
}

#[test]
fn rejects_manual_attention_comments_with_private_or_unsupported_fields() {
	for (field_name, value, expected_error) in [
		(
			"failed_command",
			"cargo test --manifest-path /Users/example/repo/Cargo.toml",
			"`failed_command` must be public/team-visible text; host-local paths are not allowed.",
		),
		(
			"raw_error",
			"Missing GITHUB_PAT_Y.",
			"`raw_error` must be public/team-visible text; credential-like names are not allowed.",
		),
		(
			"next_action",
			"inspect /Users/example/repo and recover manually",
			"`next_action` must be public/team-visible text; host-local paths are not allowed.",
		),
		(
			"blockers",
			"local checkout at /Users/example/repo blocked validation",
			"`blockers` must be public/team-visible text; host-local paths are not allowed.",
		),
		(
			"evidence",
			"selected account user@example.com failed",
			"`evidence` must be public/team-visible text; private identity details are not allowed.",
		),
		(
			"summary",
			"Missing GITHUB_PAT_Y blocked automation.",
			"`summary` must be public/team-visible text; credential-like names are not allowed.",
		),
	] {
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
		let label_response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_LABEL_ADD_TOOL_NAME,
			serde_json::json!({ "label": "decodex:needs-attention" }),
		);
		let mut args = tests::manual_attention_comment_args();

		args[field_name] = if matches!(field_name, "blockers" | "evidence") {
			serde_json::json!([value])
		} else {
			serde_json::json!(value)
		};

		let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

		assert!(label_response.success);
		assert!(!response.success, "comment should be rejected for {field_name}");
		assert!(tracker.comments.borrow().is_empty());
		assert!(
			tracker.label_additions.borrow().is_empty(),
			"rejected manual-attention comment must not leave a needs-attention label"
		);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText { text: String::from(expected_error) }]
		);
	}
}

#[test]
fn rejects_manual_attention_comment_with_runtime_owned_error_class() {
	for error_class in [
		"retryable_execution_failure",
		"repo_gate_verify_failed",
		"repo_gate_baseline_failed",
		"repo_gate_preexisting_baseline_failed",
		"repo_gate_global_baseline_failed",
		"repository_wide_docs_okf_check_failed",
		"pre_existing_docs_gate_failed",
		"repo_gate_git_lock_contention",
		"stalled_run_detected",
		"app_server_plugin_list_timeout",
		"app_server_turn_failed",
		"app_server_usage_limit_exceeded",
		"app_server_dynamic_tool_failed",
		"phase_goal_terminal_path_missing",
	] {
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
		let label_response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_LABEL_ADD_TOOL_NAME,
			serde_json::json!({ "label": "decodex:needs-attention" }),
		);
		let mut args = tests::manual_attention_comment_args();

		args["error_class"] = serde_json::json!(error_class);

		let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

		assert!(label_response.success);
		assert!(
			!response.success,
			"manual attention should reject runtime-owned class {error_class}"
		);
		assert!(tracker.comments.borrow().is_empty());
		assert!(
			tracker.label_additions.borrow().is_empty(),
			"runtime-owned class {error_class} must not leave a needs-attention label"
		);
		assert!(matches!(
			response.content_items.as_slice(),
			[DynamicToolContentItem::InputText { text }]
				if text.contains("cannot use runtime-owned error class")
					&& text.contains(error_class)
		));
	}
}

#[test]
fn rejects_unsupported_issue_comment_kinds_without_mutation() {
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
	let label_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		serde_json::json!({
			"kind": "status_note",
			"error_class": "operator_decision_required",
			"next_action": "resolve manually",
			"blockers": ["operator decision is required"],
			"evidence": ["agent attempted unsupported comment kind"]
		}),
	);

	assert!(label_response.success);
	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"unsupported comment kind must not leave a needs-attention label"
	);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Unsupported `issue_comment` kind `status_note`")
	));
}

#[test]
fn rejects_manual_attention_comment_before_needs_attention_label() {
	let issue = tests::sample_issue();
	let tracker = FakeTracker::new();
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
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires a successful `issue_label_add` call")
	));
}

#[test]
fn rejects_manual_attention_comment_with_non_public_error_class_shape() {
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
	let label_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let mut args = tests::manual_attention_comment_args();

	args["error_class"] = serde_json::json!("Missing-GITHUB_PAT_Y");

	let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

	assert!(label_response.success);
	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"invalid error_class shape must not leave a needs-attention label"
	);
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: String::from("`error_class` must be a public snake_case identifier.")
		}]
	);
}

#[test]
fn rejects_legacy_public_comments_with_sensitive_or_unknown_paths() {
	for (body, expected_error) in [
		(
			"decodex run failed and will retry\n\n- worktree_path: `/absolute/path/to/repo/.worktrees/DEC-1`",
			"`worktree_path` must be repository-relative, not `/absolute/path/to/repo/.worktrees/DEC-1`.",
		),
		(
			"decodex run failed and will retry\n\n- unexpected_path: `/absolute/path/to/repo/.worktrees/DEC-1`",
			"Unsupported structured field `unexpected_path` in public issue comments.",
		),
		(
			"decodex run failed and will retry\n\n- worktree_path: `C:/absolute/path/to/repo/.worktrees/DEC-1`",
			"`body` must be public/team-visible text; host-local paths are not allowed.",
		),
	] {
		let error = public_text::validate_public_comment_body(body)
			.expect_err("legacy free-form body should still fail public text validation");

		assert_eq!(error, expected_error);
	}
}

#[test]
fn records_manual_attention_label_intent_without_linear_mutation() {
	let issue = tests::sample_issue();
	let tracker = FakeTracker::new();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);

	assert!(response.success);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Manual-attention label intent recorded")
	));
}

#[test]
fn adds_allowed_workflow_opt_out_label() {
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:manual-only" }),
	);

	assert!(response.success);
	assert_eq!(tracker.label_additions.borrow().as_slice(), [vec![String::from("label-manual")]]);
}

#[test]
fn rejects_invalid_label_add_argument_shapes() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let cases = [
		(
			serde_json::json!({ "unexpected_label_field": "decodex:needs-attention" }),
			"Invalid `issue.label.add` arguments: missing field `label`",
		),
		(
			serde_json::json!({
				"label": "decodex:needs-attention",
				"unexpected_label_field": "decodex:needs-attention",
			}),
			"Invalid `issue.label.add` arguments: unknown field `unexpected_label_field`",
		),
	];

	for (arguments, message) in cases {
		let response =
			DynamicToolHandler::handle_call(&bridge, ISSUE_LABEL_ADD_TOOL_NAME, arguments);

		assert!(!response.success);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText { text: String::from(message) }]
		);
	}

	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
}
