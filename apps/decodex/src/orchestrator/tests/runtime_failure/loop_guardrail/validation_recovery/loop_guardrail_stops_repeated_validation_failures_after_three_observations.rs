use crate::orchestrator::{
	RepoGateFailure,
	tests::{
		self,
		runtime_failure::{self, RepoGateFailureKind, Report, StateStore, orchestrator},
	},
};

#[test]
fn loop_guardrail_stops_repeated_validation_failures_after_three_observations() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let error = || {
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("Repo verify command `cargo make test` failed: same assertion failed"),
		))
	};

	for attempt_number in 1..=2 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error(),
			)
			.expect("guardrail observation should persist")
			.is_none(),
			"guardrail should allow repair attempt {attempt_number}"
		);
	}

	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error(),
	)
	.expect("third matching failure should evaluate")
	.expect("third matching validation failure should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::ValidationRepeat);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class.as_deref(), Some("repo_gate_verify_failed"));

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
		.expect("validation checkpoint should read")
		.expect("validation checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "loop_guardrail_checkpoint");
	assert_eq!(events[0].payload()["reason"], "validation_repeat");
}
