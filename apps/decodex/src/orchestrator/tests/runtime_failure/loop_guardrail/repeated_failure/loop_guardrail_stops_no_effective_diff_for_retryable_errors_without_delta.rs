use crate::orchestrator::tests::{
	self,
	runtime_failure::{self, Report, StateStore, orchestrator},
};

#[test]
fn loop_guardrail_stops_no_effective_diff_for_retryable_errors_without_delta() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

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
	.expect("no effective diff should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::NoEffectiveDiff);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class, None);

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
		.expect("no-diff checkpoint should read")
		.expect("no-diff checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);
}
