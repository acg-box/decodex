use crate::{
	orchestrator::{self, IssueDispatchMode, tests::recovery_terminal_support},
	state,
};

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
