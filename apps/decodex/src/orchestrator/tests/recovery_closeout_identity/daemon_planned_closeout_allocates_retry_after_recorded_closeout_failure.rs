use crate::{
	orchestrator::{self, IssueDispatchMode, RetryQueue, tests::recovery_terminal_support},
	state,
};

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
