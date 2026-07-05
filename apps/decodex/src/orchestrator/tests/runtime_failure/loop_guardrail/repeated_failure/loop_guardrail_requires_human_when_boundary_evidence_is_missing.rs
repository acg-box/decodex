use crate::orchestrator::tests::{
	self,
	runtime_failure::{self, Report, StateStore, orchestrator},
};

#[test]
fn loop_guardrail_requires_human_when_boundary_evidence_is_missing() {
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
	let decision = orchestrator::loop_guardrail_architecture_recovery_decision(
		&config,
		&state_store,
		&issue_run,
		stop,
		&error,
	)
	.expect("architecture recovery decision should record");
	let terminal_stop = match decision {
		orchestrator::LoopGuardrailRecoveryDecision::Start(_) => {
			panic!("missing authority evidence must not start recovery")
		},
		orchestrator::LoopGuardrailRecoveryDecision::HumanRequired(stop) => stop,
	};

	assert_eq!(terminal_stop.terminal_error_class(), "contract_boundary_required");

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["policy_decision"] == "requires_human_decision"
			&& event.payload()["disposition"] == "requires_human"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["reason_code"] == "contract_boundary_required"
			&& event.payload()["authority_boundary_check"]["policy_decision"]
				== "requires_human_decision"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_terminal"
			&& event.payload()["reason_code"] == "contract_boundary_required"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "authority_decision_request"
			&& event.payload()["reason"] == "contract_boundary_required"
	}));
}
