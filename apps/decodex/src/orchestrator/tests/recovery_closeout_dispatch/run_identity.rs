use crate::{
	orchestrator::{
		self, IssueDispatchMode, RunSummary, TargetIssueRunContext,
		tests::recovery_terminal_support,
	},
	tracker::records,
};

#[test]
fn reuses_completed_handoff_run_identity() {
	let fixture = recovery_terminal_support::closeout_identity_fixture();
	let _keep_fixture_alive = (&fixture._temp_dir, &fixture._path_guard);

	recovery_terminal_support::assert_closeout_lane_ready(&fixture);

	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &fixture.tracker,
		project: &fixture.config,
		workflow: &fixture.workflow,
		state_store: &fixture.state_store,
		issue_id: &fixture.issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: false,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("direct retained closeout should run")
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
		"direct closeout must not look like a hidden retry: {message}"
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
		"direct closeout event should reuse the completed handoff identity"
	);
	assert!(
		fixture
			.state_store
			.run_attempt_for_issue_attempt(&fixture.issue.id, 2)
			.expect("second attempt lookup should succeed")
			.is_none(),
		"successful direct closeout should not create an invisible second attempt"
	);
}

#[test]
fn same_run_closeout_reuses_matching_active_handoff_lease() {
	let fixture = recovery_terminal_support::closeout_identity_fixture();
	let _keep_fixture_alive = (&fixture._temp_dir, &fixture._path_guard);

	recovery_terminal_support::assert_closeout_lane_ready(&fixture);

	fixture
		.state_store
		.upsert_lease(
			fixture.config.service_id(),
			&fixture.issue.id,
			&fixture.completed_run_id,
			"In Review",
		)
		.expect("handoff lease should recover before same-run closeout");

	let source_summary = RunSummary {
		project_id: fixture.config.service_id().to_owned(),
		issue_id: fixture.issue.id.clone(),
		issue_identifier: fixture.issue.identifier.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("Todo"),
		retry_project_slug: fixture.config.service_id().to_owned(),
		dispatch_mode: IssueDispatchMode::Normal,
		branch_name: fixture.worktree.branch_name.clone(),
		worktree_path: fixture.worktree.path.clone(),
		attempt_number: 1,
		run_id: fixture.completed_run_id.clone(),
		continuation_pending: false,
		program_dispatch: None,
	};
	let summary = orchestrator::run_retained_closeout_for_handoff_summary(
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
		&source_summary,
	)
	.expect("same-run retained closeout should run")
	.expect("same-run retained closeout should produce a summary");

	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, fixture.completed_run_id);
	assert!(
		fixture
			.state_store
			.lease_for_issue(&fixture.issue.id)
			.expect("lease lookup should succeed")
			.is_none(),
		"same-run closeout should clear the recovered handoff lease"
	);
}
