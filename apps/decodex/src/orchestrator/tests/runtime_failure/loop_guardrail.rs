use super::{
	AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition,
	AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface, LoopGuardrailReason,
	LoopGuardrailStopRequested, RepoGateFailureKind, Report, StateStore, add_origin_remote,
	checkout_new_branch, commit_worktree_change, fs, git_output, loop_guardrail_issue_run,
	orchestrator, sample_issue, temp_project_layout, temp_project_layout_with_read_first,
};

#[test]
fn loop_guardrail_stops_repeated_validation_failures_after_three_observations() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let error = || {
		Report::new(orchestrator::RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("Repo verify command `cargo make test` failed: same assertion failed"),
		))
	};

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);

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

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
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

#[test]
fn loop_guardrail_starts_architecture_recovery_when_boundary_is_within_authority() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let error = || {
		Report::new(orchestrator::RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("Repo verify command `cargo make test` failed: same assertion failed"),
		))
	};

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);

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

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
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

#[test]
fn loop_guardrail_review_churn_blocks_landing_but_continues_recovery() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 1);

	state_store
		.append_private_execution_event(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			1,
			"review_checkpoint",
			serde_json::json!({
				"phase": "handoff",
				"status": "findings",
				"head_sha": "review-head",
				"nonclean_rounds": 3,
				"review": {
					"accepted_findings": [{
						"summary": "Accepted reviewer finding that exhausted review churn."
					}],
					"rejected_findings": [{
						"summary": "Rejected non-current reviewer comment."
					}],
					"finding_route_summary": {
						"route_counts": [
							{"route": "current_blocker", "count": 1},
							{"route": "risk_note", "count": 1}
						],
						"next_action": "Stop repair churn and run architecture recovery."
					}
				}
			}),
		)
		.expect("review checkpoint evidence should record");

	let stop = LoopGuardrailStopRequested {
		issue_identifier: issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		reason: LoopGuardrailReason::ReviewChurn,
		consecutive_count: 3,
		fingerprint: String::from("review-head:3"),
		source_error_class: Some(String::from("review_policy_exhausted")),
		architecture_recovery_reason_code: None,
	};
	let error = Report::new(LoopGuardrailStopRequested {
		issue_identifier: issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		reason: LoopGuardrailReason::ReviewChurn,
		consecutive_count: 3,
		fingerprint: String::from("review-head:3"),
		source_error_class: Some(String::from("review_policy_exhausted")),
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

	assert!(
		matches!(decision, orchestrator::LoopGuardrailRecoveryDecision::Start(_)),
		"review-policy churn should continue recovery while blocking landing"
	);

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 1)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["policy_decision"] == "block_landing"
			&& event.payload()["policy"]["blocks_landing"] == true
			&& event.payload()["changed_surfaces"][0]["surface"] == "review_policy"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["reason_code"] == "architecture_recovery_started"
			&& event.payload()["authority_boundary_check"]["policy_decision"] == "block_landing"
			&& event.payload()["authority_boundary_check"]["blocks_landing"] == true
			&& event.payload()["review_findings"]["route_counts"][0]["route"] == "current_blocker"
			&& event.payload()["review_findings"]["route_counts"][0]["count"] == 1
			&& event.payload()["review_findings"]["route_counts"][1]["route"] == "risk_note"
			&& event.payload()["review_findings"]["route_next_action"]
				== "Stop repair churn and run architecture recovery."
	}));
}

#[test]
fn authority_boundary_infers_public_api_diff_requires_enhanced_evidence() {
	let (_temp_dir, config, _workflow) = temp_project_layout_with_read_first(
		&[("apps/decodex/src/cli.rs", "pub fn run() {}\n")],
		"Follow the repository policy.\n",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 1);
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
	let error = Report::new(orchestrator::RepoGateFailure::new(
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
		assert_architecture_recovery_diff_surface_policy(
			relative_path,
			expected_surface,
			expected_policy_decision,
		);
	}
}

fn assert_architecture_recovery_diff_surface_policy(
	relative_path: &str,
	expected_surface: AuthorityBoundarySurface,
	expected_policy_decision: AuthorityBoundaryPolicyDecision,
) {
	let (_temp_dir, config, _workflow) = temp_project_layout_with_read_first(
		&[(relative_path, "initial\n")],
		"Follow the repository policy.\n",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 1);

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
	let error = Report::new(orchestrator::RepoGateFailure::new(
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

#[test]
fn authority_boundary_public_api_surface_requires_enhanced_evidence() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 1);
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
		AuthorityBoundarySurface::Docs,
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 1);
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

#[test]
fn loop_guardrail_requires_human_when_boundary_evidence_is_missing() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
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

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
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

#[test]
fn loop_guardrail_stops_validation_repeat_when_validation_text_changes() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::new(orchestrator::RepoGateFailure::new(
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

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
	let error = Report::new(orchestrator::RepoGateFailure::new(
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

#[test]
fn loop_guardrail_stops_no_effective_diff_for_retryable_errors_without_delta() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
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

	let issue_run = loop_guardrail_issue_run(&config, &issue, 3);
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

#[test]
fn loop_guardrail_does_not_classify_committed_branch_delta_as_no_effective_diff() {
	let (temp_dir, config, _workflow) = temp_project_layout();
	let remote_root = temp_dir.path().join("origin.git");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	add_origin_remote(config.repo_root(), &remote_root);
	checkout_new_branch(config.repo_root(), "x/pubfi-pub-101");
	commit_worktree_change(
		config.repo_root(),
		"ready.txt",
		"implementation complete\n",
		"implement issue scope",
	);

	assert_eq!(git_output(config.repo_root(), &["status", "--porcelain"]), "");

	for attempt_number in 1..=3 {
		let issue_run = loop_guardrail_issue_run(&config, &issue, attempt_number);
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
