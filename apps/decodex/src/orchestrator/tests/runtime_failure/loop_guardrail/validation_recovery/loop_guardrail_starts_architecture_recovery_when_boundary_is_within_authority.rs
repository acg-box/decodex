use crate::orchestrator::{
	RepoGateFailure,
	tests::{
		self,
		runtime_failure::{self, RepoGateFailureKind, Report, StateStore, orchestrator},
	},
};

#[test]
fn loop_guardrail_starts_architecture_recovery_when_boundary_is_within_authority() {
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
			.is_none()
		);
	}

	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let error = error();
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third matching failure should evaluate")
	.expect("third matching validation failure should stop");
	let decision = orchestrator::loop_guardrail_architecture_recovery_decision(
		&config,
		&state_store,
		&issue_run,
		stop,
		&error,
	)
	.expect("architecture recovery decision should record");
	let recovery = match decision {
		orchestrator::LoopGuardrailRecoveryDecision::Start(recovery) => recovery,
		orchestrator::LoopGuardrailRecoveryDecision::HumanRequired(_) => {
			panic!("repo-gate validation repeat should recover autonomously")
		},
	};

	assert_eq!(recovery.attempt_number, 1);
	assert!(recovery.detail.contains("materially different implementation strategy"));
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
			.expect("checkpoint read should succeed")
			.is_none(),
		"started recovery should clear the stopped guardrail reason"
	);

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["disposition"] == "within_authority"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["reason_code"] == "architecture_recovery_started"
			&& event.payload()["authority_boundary_check"]["disposition"] == "within_authority"
			&& event.payload()["retained_worktree"]["tracked_status"].is_string()
			&& event.payload()["validation_failures"]["source_error_class"]
				== "repo_gate_verify_failed"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_started"
			&& event.payload()["next_strategy"] == "materially_different_architecture_recovery"
	}));
}
