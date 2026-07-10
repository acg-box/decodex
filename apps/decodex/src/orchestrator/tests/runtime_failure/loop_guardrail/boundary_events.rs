use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		self, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
		AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
		LoopGuardrailReason, LoopGuardrailStopRequested, Report, StateStore, orchestrator,
	},
};

#[test]
fn authority_boundary_public_api_surface_requires_enhanced_evidence() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 1);
	let event = orchestrator::record_authority_boundary_check_private_event(
		&state_store,
		AuthorityBoundaryCheckInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
			decision_contract_ids: Vec::new(),
			attempted_recovery_reason: "validation_repeat",
			changed_surfaces: vec![AuthorityBoundaryChangedSurface {
				surface: AuthorityBoundarySurface::PublicApi,
				change_summary: "Public API behavior may change.",
				policy_decision: AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
				legacy_disposition: AuthorityBoundaryDisposition::WithinAuthority,
			}],
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
			disposition: AuthorityBoundaryDisposition::WithinAuthority,
			final_disposition_reason: "Public API changes require enhanced evidence before landing.",
			improvement_signals: Vec::new(),
		},
	)
	.expect("authority boundary event should persist");

	assert_eq!(event.payload()["policy_decision"], "requires_enhanced_evidence");
	assert_eq!(event.payload()["policy"]["allows_autonomous_recovery"], true);
	assert_eq!(event.payload()["policy"]["requires_enhanced_evidence"], true);
	assert_eq!(event.payload()["policy"]["blocks_landing"], false);
	assert_eq!(event.payload()["changed_surfaces"][0]["surface"], "public_api");
}

#[test]
fn authority_boundary_surface_policy_matrix_classifies_risk() {
	for surface in [
		AuthorityBoundarySurface::ImplementationStrategy,
		AuthorityBoundarySurface::Runtime,
		AuthorityBoundarySurface::Tests,
	] {
		assert_eq!(surface.policy_decision(), AuthorityBoundaryPolicyDecision::AutoContinue);
	}
	for surface in [
		AuthorityBoundarySurface::PublicApi,
		AuthorityBoundarySurface::Config,
		AuthorityBoundarySurface::Security,
		AuthorityBoundarySurface::Data,
		AuthorityBoundarySurface::Billing,
		AuthorityBoundarySurface::Privacy,
	] {
		assert_eq!(
			surface.policy_decision(),
			AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence
		);
	}
	for surface in [AuthorityBoundarySurface::Validation, AuthorityBoundarySurface::ReviewPolicy] {
		assert_eq!(surface.policy_decision(), AuthorityBoundaryPolicyDecision::BlockLanding);
	}
	for surface in [
		AuthorityBoundarySurface::Objective,
		AuthorityBoundarySurface::NonGoal,
		AuthorityBoundarySurface::ExternalDependency,
		AuthorityBoundarySurface::RetainedOwnership,
		AuthorityBoundarySurface::AuthorityEvidence,
	] {
		assert_eq!(
			surface.policy_decision(),
			AuthorityBoundaryPolicyDecision::RequiresHumanDecision
		);
	}

	assert_eq!(
		AuthorityBoundaryPolicyDecision::AutoContinue.disposition(),
		AuthorityBoundaryDisposition::WithinAuthority
	);
	assert_eq!(
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence.disposition(),
		AuthorityBoundaryDisposition::WithinAuthority
	);
	assert_eq!(
		AuthorityBoundaryPolicyDecision::BlockLanding.disposition(),
		AuthorityBoundaryDisposition::WithinAuthority
	);
	assert_eq!(
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision.disposition(),
		AuthorityBoundaryDisposition::RequiresHuman
	);
	assert!(AuthorityBoundaryPolicyDecision::AutoContinue.allows_autonomous_recovery());
	assert!(AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence.allows_autonomous_recovery());
	assert!(AuthorityBoundaryPolicyDecision::BlockLanding.allows_autonomous_recovery());
	assert!(!AuthorityBoundaryPolicyDecision::RequiresHumanDecision.allows_autonomous_recovery());
	assert!(AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence.requires_enhanced_evidence());
	assert!(AuthorityBoundaryPolicyDecision::BlockLanding.requires_enhanced_evidence());
	assert!(AuthorityBoundaryPolicyDecision::BlockLanding.blocks_landing());
}

#[test]
fn loop_guardrail_uncovered_direction_requires_human_decision() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 1);
	let stop = LoopGuardrailStopRequested {
		issue_identifier: issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		reason: LoopGuardrailReason::UncoveredDirection,
		consecutive_count: 3,
		fingerprint: String::from("uncovered-direction"),
		source_error_class: Some(String::from("research_contract_required")),
		architecture_recovery_reason_code: None,
	};
	let error = Report::new(LoopGuardrailStopRequested {
		issue_identifier: issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		reason: LoopGuardrailReason::UncoveredDirection,
		consecutive_count: 3,
		fingerprint: String::from("uncovered-direction"),
		source_error_class: Some(String::from("research_contract_required")),
		architecture_recovery_reason_code: None,
	});
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
			panic!("objective-changing recovery must require a human decision")
		},
		orchestrator::LoopGuardrailRecoveryDecision::HumanRequired(stop) => stop,
	};

	assert_eq!(terminal_stop.terminal_error_class(), "contract_boundary_required");

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 1)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["policy_decision"] == "requires_human_decision"
			&& event.payload()["changed_surfaces"][0]["surface"] == "objective"
			&& event.payload()["changed_surfaces"][0]["policy_decision"]
				== "requires_human_decision"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "authority_decision_request"
			&& event.payload()["reason"] == "contract_boundary_required"
	}));
}
