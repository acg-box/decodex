use crate::{
	orchestrator::{
		self, IssueDispatchMode, RetryQueue, ReviewOrchestrationMarker,
		tests::{
			TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT, TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
			TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT, recovery_terminal_support, {self},
		},
	},
	state,
	tracker::records,
};

#[test]
fn run_project_once_closeout_reuses_completed_handoff_run_identity_for_record_and_summary() {
	let fixture = recovery_terminal_support::closeout_identity_fixture();
	let _keep_fixture_alive = (&fixture._temp_dir, &fixture._path_guard);

	recovery_terminal_support::assert_closeout_lane_ready(&fixture);

	let planned = orchestrator::run_project_once(
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
		true,
	)
	.expect("retained closeout dry-run planning should succeed");
	let planned =
		planned.expect("retained closeout should be selected before deterministic execution");

	assert_eq!(planned.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(planned.run_id, fixture.completed_run_id);
	assert_eq!(planned.attempt_number, 1);

	let summary = orchestrator::run_project_once(
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
		false,
	)
	.expect("retained closeout should run")
	.expect("closeout summary should be printed");
	let message = orchestrator::format_run_once_summary(&summary, false);

	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, fixture.completed_run_id);
	assert_eq!(summary.attempt_number, 1);
	assert!(
		message.contains(&format!("run_id={}", fixture.completed_run_id)),
		"terminal summary should print the completed handoff run id: {message}"
	);
	assert!(
		!message.contains("attempt-2"),
		"successful closeout must not look like a hidden retry: {message}"
	);

	let issue_comments = fixture.tracker.issue_comments.borrow();
	let closeout_comments =
		issue_comments.get(&fixture.issue.id).expect("closeout should write an issue comment");

	assert!(
		closeout_comments.iter().any(|comment| {
			records::parse_linear_execution_event_record(&comment.body).is_some_and(|record| {
				record.event_type == "closeout"
					&& record.run_id == fixture.completed_run_id
					&& record.attempt_number == 1
					&& record.branch.as_deref() == Some(fixture.worktree.branch_name.as_str())
					&& record.pr_url.as_deref() == Some(fixture.pr_url.as_str())
			})
		}),
		"closeout event should reuse the completed handoff identity"
	);
	assert_eq!(
		fixture
			.state_store
			.run_attempt(&fixture.completed_run_id)
			.expect("run attempt lookup should succeed")
			.expect("completed handoff attempt should remain recorded")
			.status(),
		"succeeded"
	);
	assert!(
		fixture
			.state_store
			.run_attempt_for_issue_attempt(&fixture.issue.id, 2)
			.expect("second attempt lookup should succeed")
			.is_none(),
		"successful closeout should not create an invisible second attempt"
	);
	assert_eq!(
		fixture
			.state_store
			.next_attempt_number(&fixture.issue.id)
			.expect("next attempt lookup should succeed"),
		2,
		"the store should only know about the completed first attempt"
	);
}

#[test]
fn daemon_planned_closeout_reuses_handoff_identity_after_parent_failed_status() {
	let fixture = recovery_terminal_support::closeout_identity_fixture();
	let _keep_fixture_alive = (&fixture._temp_dir, &fixture._path_guard);
	let mut retry_queue = RetryQueue::default();

	recovery_terminal_support::assert_closeout_lane_ready(&fixture);

	fixture
		.state_store
		.update_run_status(&fixture.completed_run_id, "failed")
		.expect("daemon parent failed status should record");

	tests::seed_review_orchestration_marker_for_path(
		&fixture.state_store,
		fixture.config.service_id(),
		&fixture.worktree.path,
		&ReviewOrchestrationMarker::new(
			&fixture.completed_run_id,
			1,
			&fixture.worktree.branch_name,
			&fixture.pr_url,
			&fixture.head_oid,
			"waiting_for_merge",
			Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID),
			Some(TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT),
			Some(0),
			0,
			1,
			Some(TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT),
		),
	);

	let (summary, from_retry_queue) = orchestrator::plan_next_daemon_run(
		&mut retry_queue,
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
	)
	.expect("daemon planning should succeed")
	.expect("retained closeout should be selected");

	assert!(!from_retry_queue, "status-visible closeout should not consume a retry claim");
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, fixture.completed_run_id);
	assert_eq!(summary.attempt_number, 1);
	assert!(
		fixture
			.state_store
			.try_acquire_lease(
				fixture.config.service_id(),
				&summary.issue_id,
				&summary.run_id,
				&summary.issue_state,
			)
			.expect("daemon parent should acquire the planned closeout lease")
	);

	let daemon_spawn_state = orchestrator::materialize_daemon_spawn_state(
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
		&summary,
	)
	.expect("daemon parent should materialize planned closeout state");

	fixture
		.state_store
		.record_run_attempt(&summary.run_id, &summary.issue_id, summary.attempt_number, "starting")
		.expect("daemon parent should record the planned closeout attempt");
	fixture
		.state_store
		.upsert_worktree(
			fixture.config.service_id(),
			&summary.issue_id,
			&daemon_spawn_state.worktree.branch_name,
			&daemon_spawn_state.worktree.path.display().to_string(),
		)
		.expect("daemon parent should retain closeout worktree mapping");

	let handoff_attempt = fixture
		.state_store
		.run_attempt(&fixture.completed_run_id)
		.expect("handoff attempt lookup should succeed")
		.expect("handoff attempt should still exist");

	assert_eq!(handoff_attempt.attempt_number(), 1);
	assert_eq!(handoff_attempt.status(), "starting");
	assert!(
		fixture
			.state_store
			.run_attempt_for_issue_attempt(&fixture.issue.id, 2)
			.expect("second attempt lookup should succeed")
			.is_none(),
		"daemon planning must not materialize a synthetic attempt 2"
	);
}

#[test]
fn daemon_planned_closeout_allocates_retry_after_recorded_closeout_failure() {
	let fixture = recovery_terminal_support::closeout_identity_fixture();
	let _keep_fixture_alive = (&fixture._temp_dir, &fixture._path_guard);
	let mut retry_queue = RetryQueue::default();

	recovery_terminal_support::assert_closeout_lane_ready(&fixture);

	fixture
		.state_store
		.update_run_status(&fixture.completed_run_id, "failed")
		.expect("failed closeout attempt should record");

	state::write_run_retry_schedule(
		&fixture.worktree.path,
		&fixture.completed_run_id,
		1,
		"failure",
		12_345,
	)
	.expect("failed closeout retry schedule should write");

	let (summary, from_retry_queue) = orchestrator::plan_next_daemon_run(
		&mut retry_queue,
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
	)
	.expect("daemon planning should succeed")
	.expect("retained closeout should be selected");

	assert!(!from_retry_queue, "normal planner fallback should not consume a retry claim");
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.attempt_number, 2);
	assert_ne!(summary.run_id, fixture.completed_run_id);
}

#[test]
fn run_project_once_closeout_preserves_handoff_identity_after_fresh_activity_recovery() {
	let fixture = recovery_terminal_support::closeout_identity_fixture();
	let _keep_fixture_alive = (&fixture._temp_dir, &fixture._path_guard);

	state::write_run_activity_marker(&fixture.worktree.path, &fixture.completed_run_id, 1)
		.expect("fresh handoff activity should write");

	let summary = orchestrator::run_project_once(
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
		false,
	)
	.expect("retained closeout should run after recovery")
	.expect("closeout summary should be printed");
	let message = orchestrator::format_run_once_summary(&summary, false);

	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, fixture.completed_run_id);
	assert_eq!(summary.attempt_number, 1);
	assert!(
		!message.contains("attempt-2"),
		"recovered handoff closeout must not report a synthetic retry: {message}"
	);
	assert_eq!(
		fixture
			.state_store
			.run_attempt(&fixture.completed_run_id)
			.expect("run attempt lookup should succeed")
			.expect("completed handoff attempt should remain recorded")
			.status(),
		"succeeded"
	);
	assert!(
		fixture
			.state_store
			.run_attempt_for_issue_attempt(&fixture.issue.id, 2)
			.expect("second attempt lookup should succeed")
			.is_none(),
		"recovered closeout should not create an invisible second attempt"
	);
	assert!(
		fixture
			.state_store
			.lease_for_issue(&fixture.issue.id)
			.expect("lease lookup should succeed")
			.is_none(),
		"successful recovered closeout should not leave the rebuilt handoff lease"
	);
}
