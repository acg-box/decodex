use crate::orchestrator::{
	RepoGateFailure,
	tests::{
		self,
		runtime_failure::{self, RepoGateFailureKind, Report, StateStore, orchestrator},
	},
};

#[test]
fn loop_guardrail_stops_validation_repeat_when_validation_text_changes() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			format!("Repo verify command failed with assertion variant {attempt_number}"),
		));

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
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::VerifyCommandFailed,
		String::from("Repo verify command failed with assertion variant 3"),
	));
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third unchanged delta should evaluate")
	.expect("normalized validation repeat should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::ValidationRepeat);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class.as_deref(), Some("repo_gate_verify_failed"));

	let validation_checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
		.expect("validation checkpoint should read")
		.expect("validation checkpoint should exist");

	assert_eq!(validation_checkpoint.consecutive_count(), 3);
	assert!(
		validation_checkpoint
			.fingerprint()
			.contains("repo_gate_verify_failed:repo_gate:validation_repair"),
		"normalized fingerprint should not include raw error text"
	);
}
