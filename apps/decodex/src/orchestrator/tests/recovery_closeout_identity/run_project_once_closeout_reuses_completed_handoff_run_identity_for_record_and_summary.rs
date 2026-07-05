use crate::{
	orchestrator::{self, IssueDispatchMode, tests::recovery_terminal_support},
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
