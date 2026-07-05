use crate::orchestrator::tests::{
	self,
	runtime_failure::{self, FakeTracker, Report, ReviewHandoffMarker, StateStore, orchestrator},
};

#[test]
fn handle_failure_requires_rebind_when_handoff_marker_head_ref_mismatches_without_checkpoint() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let head_oid = runtime_failure::git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let handoff = ReviewHandoffMarker::new(
		&issue_run.run_id,
		issue_run.attempt_number,
		&issue_run.worktree.branch_name,
		"https://github.com/hack-ink/decodex/pull/957",
		"main",
		"x/pubfi-pub-101-stale",
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
	.expect("mismatched handoff marker should require explicit rebind attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert_eq!(
		tracker.label_additions.borrow().last(),
		Some(&(issue.id.clone(), vec![String::from("label-needs-attention")]))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("review_handoff_state_drift")
			&& comment.contains("restore or rebind the post-review lifecycle")
	}));
	assert!(
		tracker.comments.borrow().iter().all(|comment| { !comment.contains("no_effective_diff") })
	);
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"untrusted handoff marker must not fall through to no effective diff"
	);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "review_handoff_state_drift_detected"
			&& event.payload()["reason"] == "review_handoff_marker_pr_head_ref_mismatch"
			&& event.payload()["checkpoint_status"].is_null()
	}));
}

#[test]
fn handle_failure_requires_rebind_when_handoff_marker_issue_state_is_unsupported() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("Backlog", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let head_oid = runtime_failure::git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let handoff = ReviewHandoffMarker::new(
		&issue_run.run_id,
		issue_run.attempt_number,
		&issue_run.worktree.branch_name,
		"https://github.com/hack-ink/decodex/pull/957",
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
	.expect("unsupported issue state should require explicit handoff recovery");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("review_handoff_state_drift")
			&& comment.contains("restore or rebind the post-review lifecycle")
	}));
	assert!(
		tracker.comments.borrow().iter().all(|comment| { !comment.contains("no_effective_diff") })
	);

	let events = state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "review_handoff_state_drift_detected"
			&& event.payload()["reason"] == "review_handoff_marker_issue_state_unsupported"
	}));
}
