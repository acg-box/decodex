use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		self, FakeTracker, Report, ReviewPolicyCheckpointInput, StateStore, orchestrator,
	},
};

#[test]
fn handle_failure_requires_rebind_when_clean_handoff_checkpoint_has_no_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let head_oid = runtime_failure::git_output(config.repo_root(), &["rev-parse", "HEAD"]);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: &head_oid,
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("clean handoff checkpoint should persist");

	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("child exited without useful change"),
	)
	.expect("missing handoff marker should require explicit rebind attention");

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
		"missing handoff marker must not be reclassified as no effective diff"
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
			&& event.payload()["reason"] == "missing_review_handoff_marker"
			&& event.payload()["checkpoint_status"] == "clean"
	}));
}
