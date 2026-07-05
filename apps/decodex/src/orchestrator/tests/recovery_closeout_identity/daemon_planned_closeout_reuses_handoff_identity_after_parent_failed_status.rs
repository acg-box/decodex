use crate::orchestrator::{
	self, IssueDispatchMode, RetryQueue, ReviewOrchestrationMarker,
	tests::{
		TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT, TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT, recovery_terminal_support, {self},
	},
};

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
