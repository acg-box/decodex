use crate::orchestrator::tests::{
	self,
	runtime_failure::{self, Report, StateStore, orchestrator},
};

#[test]
fn loop_guardrail_does_not_classify_committed_branch_delta_as_no_effective_diff() {
	let (temp_dir, config, _workflow) = tests::temp_project_layout();
	let remote_root = temp_dir.path().join("origin.git");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	runtime_failure::add_origin_remote(config.repo_root(), &remote_root);
	runtime_failure::checkout_new_branch(config.repo_root(), "x/pubfi-pub-101");
	runtime_failure::commit_worktree_change(
		config.repo_root(),
		"ready.txt",
		"implementation complete\n",
		"implement issue scope",
	);

	assert_eq!(runtime_failure::git_output(config.repo_root(), &["status", "--porcelain"]), "");

	for attempt_number in 1..=3 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited after committed issue-scoped work");
		let stop = orchestrator::retryable_failure_loop_guardrail_stop(
			&config,
			&state_store,
			&issue_run,
			&error,
		)
		.expect("guardrail observation should evaluate");

		assert!(stop.is_none(), "committed branch delta must not be reported as no_effective_diff");
	}

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("checkpoint lookup should succeed")
			.is_none()
	);
}
