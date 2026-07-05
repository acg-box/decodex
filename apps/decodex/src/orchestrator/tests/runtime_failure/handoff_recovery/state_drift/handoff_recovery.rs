use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		self, FakeTracker, Report, ReviewHandoffMarker, StateStore, orchestrator, tracker,
	},
};

#[test]
fn handle_failure_recovers_review_handoff_state_drift_before_no_effective_diff_terminalization() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/957";

	for attempt_number in 1..=2 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited without useful change");

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let head_oid = runtime_failure::git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let handoff = ReviewHandoffMarker::new(
		&issue_run.run_id,
		issue_run.attempt_number,
		&issue_run.worktree.branch_name,
		pr_url,
		"main",
		&issue_run.worktree.branch_name,
		&head_oid,
	);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_review_handoff_marker(config.service_id(), &issue.id, &handoff)
		.expect("review handoff marker should record");

	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("child exited without useful change"),
	)
	.expect("review handoff drift should recover before no-diff terminalization");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-review")))
	);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().all(|comment| {
		!comment.contains("decodex run failed and needs attention")
			&& !comment.contains("no_effective_diff")
	}));
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"handoff recovery owns the lifecycle and clears stale retry/no-diff guardrails"
	);

	let run_attempt = state_store
		.run_attempt(&issue_run.run_id)
		.expect("run attempt should read")
		.expect("run attempt should remain present");

	assert_eq!(run_attempt.status(), "succeeded");

	let orchestration = state_store
		.review_orchestration_marker(config.service_id(), &issue.id, &handoff)
		.expect("review orchestration should read")
		.expect("review orchestration should be rebound");

	assert_eq!(orchestration.phase(), "request_pending");
	assert_eq!(orchestration.head_sha(), head_oid);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "review_handoff_state_drift_recovered"
			&& event.payload()["reason"] == "current_review_handoff_marker"
			&& event.payload()["target_issue_state"] == "In Review"
	}));
}
