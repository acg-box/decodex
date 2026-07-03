mod authority_boundary;
mod boundary_events;
mod repeated_failure;
mod validation_recovery;

use crate::orchestrator::{
	RepoGateFailure,
	tests::{
		self,
		runtime_failure::{
			self, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface, LoopGuardrailReason,
			LoopGuardrailStopRequested, RepoGateFailureKind, Report, StateStore, fs, orchestrator,
		},
	},
};

fn assert_architecture_recovery_diff_surface_policy(
	relative_path: &str,
	expected_surface: AuthorityBoundarySurface,
	expected_policy_decision: AuthorityBoundaryPolicyDecision,
) {
	let (_temp_dir, config, _workflow) = runtime_failure::temp_project_layout_with_read_first(
		&[(relative_path, "initial\n")],
		"Follow the repository policy.\n",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 1);

	fs::write(config.repo_root().join(relative_path), "updated\n")
		.expect("tracked file should change");

	let stop = LoopGuardrailStopRequested {
		issue_identifier: issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		reason: LoopGuardrailReason::ValidationRepeat,
		consecutive_count: 3,
		fingerprint: String::from("validation-repeat"),
		source_error_class: Some(String::from("repo_gate_verify_failed")),
		architecture_recovery_reason_code: None,
	};
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::VerifyCommandFailed,
		String::from("Repo verify command `cargo make test` failed: same assertion failed"),
	));
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
			panic!("high-risk evidence policy should not block autonomous recovery")
		},
	};

	assert_eq!(recovery.policy_decision, expected_policy_decision);

	let expected_surface_name = expected_surface.as_str();
	let expected_policy_name = expected_policy_decision.as_str();
	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 1)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["policy_decision"] == expected_policy_name
			&& event.payload()["changed_surfaces"]
				.as_array()
				.expect("changed surfaces should be an array")
				.iter()
				.any(|surface| {
					surface["surface"] == expected_surface_name
						&& surface["policy_decision"] == expected_policy_name
				})
	}));
}
