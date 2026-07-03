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

#[test]
fn authority_boundary_infers_public_api_diff_requires_enhanced_evidence() {
	let (_temp_dir, config, _workflow) = runtime_failure::temp_project_layout_with_read_first(
		&[("apps/decodex/src/cli.rs", "pub fn run() {}\n")],
		"Follow the repository policy.\n",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 1);
	let public_api_path = config.repo_root().join("apps/decodex/src/cli.rs");

	fs::write(public_api_path, "pub fn run() { println!(\"changed\"); }\n")
		.expect("tracked public API file should change");

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
			panic!("public API evidence policy should not block autonomous recovery")
		},
	};

	assert_eq!(recovery.policy_decision, AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence);
	assert!(recovery.detail.contains("requires_enhanced_evidence"));

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 1)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["policy_decision"] == "requires_enhanced_evidence"
			&& event.payload()["policy"]["requires_enhanced_evidence"] == true
			&& event.payload()["changed_surfaces"]
				.as_array()
				.expect("changed surfaces should be an array")
				.iter()
				.any(|surface| surface["surface"] == "public_api")
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_started"
			&& event.payload()["boundary_policy_decision"] == "requires_enhanced_evidence"
			&& event.payload()["requires_enhanced_evidence"] == true
	}));
}

#[test]
fn authority_boundary_infers_high_risk_diff_surface_policies() {
	for (relative_path, expected_surface, expected_policy_decision) in [
		(
			"decodex.example.toml",
			AuthorityBoundarySurface::Config,
			AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
		),
		(
			"apps/decodex/src/security/token.rs",
			AuthorityBoundarySurface::Security,
			AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
		),
		(
			"apps/decodex/src/state.rs",
			AuthorityBoundarySurface::Data,
			AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
		),
		(
			"apps/decodex/src/billing/usage.rs",
			AuthorityBoundarySurface::Billing,
			AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
		),
		(
			"apps/decodex/src/tracker/privacy_classifier.rs",
			AuthorityBoundarySurface::Privacy,
			AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
		),
		(
			"apps/decodex/src/repo_gate.rs",
			AuthorityBoundarySurface::Validation,
			AuthorityBoundaryPolicyDecision::BlockLanding,
		),
		(
			"apps/decodex/src/orchestrator/review_policy.rs",
			AuthorityBoundarySurface::ReviewPolicy,
			AuthorityBoundaryPolicyDecision::BlockLanding,
		),
	] {
		super::assert_architecture_recovery_diff_surface_policy(
			relative_path,
			expected_surface,
			expected_policy_decision,
		);
	}
}
