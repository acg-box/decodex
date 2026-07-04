use crate::orchestrator::tests::{
	self,
	runtime_failure::{self, RUN_ACTIVITY_MARKER_FILE, Report, StateStore, fs, orchestrator},
};

#[test]
fn loop_guardrail_does_not_classify_dirty_retained_diff_as_no_effective_diff() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	fs::write(config.repo_root().join("README.md"), "retained validation-ready patch\n")
		.expect("tracked file should become dirty");

	for attempt_number in 1..=3 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("phase completed local validation without terminal handoff");
		let stop = orchestrator::retryable_failure_loop_guardrail_stop(
			&config,
			&state_store,
			&issue_run,
			&error,
		)
		.expect("guardrail observation should evaluate");

		assert!(
			stop.is_none(),
			"dirty retained progress should not be reported as no_effective_diff"
		);
	}

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"no_effective_diff is reserved for retryable failures with no effective delta"
	);
}

#[test]
fn loop_guardrail_does_not_classify_untracked_retained_files_as_no_effective_diff() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	fs::write(config.repo_root().join("new-runbook.md"), "retained validation-ready file\n")
		.expect("untracked source file should write");

	for attempt_number in 1..=3 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("phase completed local validation without terminal handoff");
		let stop = orchestrator::retryable_failure_loop_guardrail_stop(
			&config,
			&state_store,
			&issue_run,
			&error,
		)
		.expect("guardrail observation should evaluate");

		assert!(
			stop.is_none(),
			"untracked retained source files should not be reported as no_effective_diff"
		);
	}

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("no-diff checkpoint should read")
			.is_none(),
		"no_effective_diff is reserved for retryable failures with no effective delta"
	);
}

#[test]
fn loop_guardrail_ignores_untracked_decodex_runtime_artifacts_for_no_effective_diff() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let control_dir = config.repo_root().join(".decodex-run-control");

	fs::write(config.repo_root().join(RUN_ACTIVITY_MARKER_FILE), "heartbeat\n")
		.expect("runtime activity marker should write");
	fs::create_dir_all(&control_dir).expect("runtime control directory should exist");
	fs::write(control_dir.join("command.json"), "{}\n").expect("runtime control file should write");

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
	let error = Report::msg("child exited without useful change");
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third no-diff observation should evaluate")
	.expect("runtime-only artifacts should still count as no effective diff");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::NoEffectiveDiff);
	assert_eq!(stop.consecutive_count, 3);
}
