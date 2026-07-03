use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		FakeTracker, Report, RetainedPartialProgress, TEST_SERVICE_ID, orchestrator, tracker,
	},
};

#[test]
fn retained_partial_progress_uses_actionable_terminal_failure_comment() {
	let error = Report::new(RetainedPartialProgress {
		issue_identifier: String::from("PUB-101"),
		run_id: String::from("pub-101-attempt-3-123"),
		worktree_path: String::from(".worktrees/PUB-101"),
		source_error_class: None,
	});
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
	);

	assert_eq!(error_class, "partial_progress_retained");
	assert!(next_action.contains("inspect retained worktree `.worktrees/PUB-101`"));
	assert!(next_action.contains("finish validation and PR handoff or reset the patch manually"));
	assert!(next_action.contains("clear label `decodex:needs-attention`"));

	let comment = orchestrator::format_terminal_failure_comment(
		"pub-101-attempt-3-123",
		3,
		String::from(".worktrees/PUB-101"),
		"x/pubfi-pub-101",
		None,
		error_class,
		&next_action,
	);

	assert!(comment.contains("decodex retained partial progress and needs attention"));
	assert!(comment.contains("- recorded_at: `"));
	assert!(!comment.contains("decodex run failed and needs attention"));
	assert!(!comment.contains("- failed_at: `"));
	assert!(comment.contains("full recovery context"));
}

#[test]
fn ensure_automation_activity_label_noops_when_active_ownership_is_confirmed() {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let mut issue = tests::sample_issue("In Progress", &[]);

	issue.labels_complete = false;

	issue.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::new(vec![issue.clone()])
		.with_label_lookup_issues(&active_label, vec![issue.clone()]);

	orchestrator::ensure_automation_activity_label(&tracker, &issue, TEST_SERVICE_ID, true).expect(
		"server-confirmed active ownership should not fail when the first label page is truncated",
	);

	assert!(
		tracker.label_updates.borrow().is_empty()
			&& tracker.label_additions.borrow().is_empty()
			&& tracker.label_removals.borrow().is_empty(),
		"server-confirmed active ownership should not trigger a label mutation"
	);

	let mut issue = tests::sample_issue("In Progress", &[active_label.as_str()]);

	issue.team.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::new(vec![issue.clone()]);

	orchestrator::ensure_automation_activity_label(&tracker, &issue, TEST_SERVICE_ID, true)
		.expect("existing active ownership should not require a paginated team-label lookup");

	assert!(
		tracker.label_updates.borrow().is_empty()
			&& tracker.label_additions.borrow().is_empty()
			&& tracker.label_removals.borrow().is_empty(),
		"no-op active-label checks should not trigger a label mutation"
	);
}

#[test]
fn ensure_automation_activity_label_uses_incremental_team_label_lookup_for_mutation() {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let mut issue = tests::sample_issue("In Progress", &[]);

	issue.labels_complete = false;

	issue.team.labels.retain(|label| label.name != active_label.as_str());

	let tracker = FakeTracker::new(vec![issue.clone()]).with_team_label_lookup_id(
		&issue.team.id,
		&active_label,
		"label-active",
	);

	orchestrator::ensure_automation_activity_label(&tracker, &issue, TEST_SERVICE_ID, true)
		.expect("active-label mutation should resolve the team label id server-side");

	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		[(issue.id.clone(), vec![String::from("label-active")])],
	);
	assert!(tracker.label_updates.borrow().is_empty());
}

#[test]
fn review_policy_terminal_failure_comments_use_runtime_owned_error_classes() {
	for (error_class, next_action) in [
		(
			"review_policy_exhausted",
			"inspect the repeated review findings and current worktree, decide the next repair or redesign manually, prepare a bounded convergence research follow-up only after the current head, review phase, non-clean round count, and validated findings are structured and machine-checkable, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		),
		(
			"architecture_review_required",
			"inspect the current findings and worktree, perform the required architecture review manually, prepare a bounded architecture research follow-up only after the current head, review phase, stop class, and architecture concern are structured and machine-checkable, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		),
		(
			"review_policy_blocked",
			"inspect the blocking condition and worktree, resolve the blocker manually, do not dispatch research unless the blocker is reclassified as a structured architecture or convergence stop, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
		),
	] {
		let comment = orchestrator::format_terminal_failure_comment(
			"pub-101-attempt-1-123",
			1,
			String::from(".worktrees/PUB-101"),
			"x/pubfi-pub-101",
			None,
			error_class,
			next_action,
		);

		assert!(comment.contains(&format!("- error_class: `{error_class}`")));
		assert!(comment.contains("Sensitive runtime details were withheld"));
	}
}
