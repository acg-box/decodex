use super::{
	ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE, ArchitectureRecoveryBoundary,
	AuthorityBoundaryChangedSurface, AuthorityBoundaryDisposition,
	AuthorityBoundaryImprovementSignal, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	DecisionContractRecord, IssueRunPlan, LoopGuardrailReason, LoopGuardrailStopRequested, Path,
	RepoGateFailure, RepoGateFailureDisposition, Report, Result, ServiceConfig, StateStore,
	git_guardrail_output,
};

pub(super) fn classify_loop_guardrail_authority_boundary(
	stop: &LoopGuardrailStopRequested,
	error: &Report,
) -> ArchitectureRecoveryBoundary {
	let source_is_repo_gate =
		stop.source_error_class.as_deref().is_some_and(|class| class.starts_with("repo_gate_"))
			|| error.downcast_ref::<RepoGateFailure>().is_some_and(|failure| {
				failure.disposition() == RepoGateFailureDisposition::ContinueRepair
			});

	match stop.reason {
		LoopGuardrailReason::ValidationRepeat | LoopGuardrailReason::RemainingDeltaUnchanged
			if source_is_repo_gate =>
			ArchitectureRecoveryBoundary {
				disposition: AuthorityBoundaryDisposition::WithinAuthority,
				policy_decision: AuthorityBoundaryPolicyDecision::AutoContinue,
				final_reason: "Repo-gate convergence failed on an engineering implementation problem; architecture recovery may change implementation strategy without weakening validation.",
				boundary_type: AuthorityBoundarySurface::ImplementationStrategy,
			},
		LoopGuardrailReason::NoEffectiveDiff if source_is_repo_gate =>
			ArchitectureRecoveryBoundary {
				disposition: AuthorityBoundaryDisposition::WithinAuthority,
				policy_decision: AuthorityBoundaryPolicyDecision::AutoContinue,
				final_reason: "No-effective-diff convergence followed repo-gate repair work; architecture recovery may replace the ineffective implementation strategy.",
				boundary_type: AuthorityBoundarySurface::ImplementationStrategy,
			},
		LoopGuardrailReason::ReviewChurn => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::WithinAuthority,
			policy_decision: AuthorityBoundaryPolicyDecision::BlockLanding,
			final_reason: "Review churn can be recovered autonomously only by changing implementation architecture while preserving accepted behavior and review standards.",
			boundary_type: AuthorityBoundarySurface::ReviewPolicy,
		},
		LoopGuardrailReason::DependencyProgramStale => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "The next viable action changes dependency or Execution Program readiness and requires accepted authority.",
			boundary_type: AuthorityBoundarySurface::ExternalDependency,
		},
		LoopGuardrailReason::UncoveredDirection => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Execution uncovered missing direction that changes the accepted Decision Contract.",
			boundary_type: AuthorityBoundarySurface::Objective,
		},
		LoopGuardrailReason::AmbiguousRetainedProgress => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Retained progress ownership is underspecified, so Decodex lacks evidence that recovery is inside authority.",
			boundary_type: AuthorityBoundarySurface::RetainedOwnership,
		},
		_ => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Guardrail evidence is insufficient to prove autonomous recovery stays inside the Authority Envelope.",
			boundary_type: AuthorityBoundarySurface::AuthorityEvidence,
		},
	}
}

pub(super) fn architecture_recovery_started_count(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<usize> {
	Ok(state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?
		.iter()
		.filter(|event| event.event_type() == ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE)
		.count())
}

pub(super) fn architecture_recovery_contracts_for_issue(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<Vec<DecisionContractRecord>> {
	let mut records = Vec::new();

	for issue_id in [&issue_run.issue.id, &issue_run.issue.identifier] {
		for record in
			state_store.list_decision_contracts_for_issue(project.service_id(), issue_id)?
		{
			if records.iter().all(|existing: &DecisionContractRecord| {
				existing.contract_id() != record.contract_id()
			}) {
				records.push(record);
			}
		}
	}

	records.sort_by(|left, right| left.contract_id().cmp(right.contract_id()));

	Ok(records)
}

pub(super) fn architecture_recovery_changed_surfaces(
	boundary: &ArchitectureRecoveryBoundary,
	worktree_path: &Path,
) -> Vec<AuthorityBoundaryChangedSurface<'static>> {
	let mut surfaces = Vec::new();

	push_architecture_recovery_changed_surface(
		&mut surfaces,
		boundary.boundary_type,
		"Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy.",
		boundary.policy_decision,
		boundary.disposition,
	);

	if let Ok(Some(diff_paths)) =
		git_guardrail_output(worktree_path, &["diff", "--name-only", "HEAD", "--"])
	{
		for relative_path in diff_paths.lines().filter(|path| !path.trim().is_empty()) {
			for surface in architecture_recovery_surfaces_for_path(relative_path) {
				push_architecture_recovery_changed_surface(
					&mut surfaces,
					surface,
					architecture_recovery_surface_summary(surface),
					surface.policy_decision(),
					surface.policy_decision().disposition(),
				);
			}
		}
	}

	surfaces
}

fn push_architecture_recovery_changed_surface(
	surfaces: &mut Vec<AuthorityBoundaryChangedSurface<'static>>,
	surface: AuthorityBoundarySurface,
	change_summary: &'static str,
	policy_decision: AuthorityBoundaryPolicyDecision,
	legacy_disposition: AuthorityBoundaryDisposition,
) {
	if surfaces.iter().any(|existing| existing.surface == surface) {
		return;
	}

	surfaces.push(AuthorityBoundaryChangedSurface {
		surface,
		change_summary,
		policy_decision,
		legacy_disposition,
	});
}

fn architecture_recovery_surfaces_for_path(relative_path: &str) -> Vec<AuthorityBoundarySurface> {
	let normalized = relative_path.replace('\\', "/");
	let lower = normalized.to_ascii_lowercase();
	let mut surfaces = Vec::new();

	if lower.starts_with("docs/") {
		surfaces.push(AuthorityBoundarySurface::Docs);

		return surfaces;
	}
	if architecture_recovery_path_is_test(&lower) {
		surfaces.push(AuthorityBoundarySurface::Tests);

		return surfaces;
	}
	if architecture_recovery_path_is_config(&lower) {
		surfaces.push(AuthorityBoundarySurface::Config);

		return surfaces;
	}
	if architecture_recovery_path_is_public_api(&lower) {
		surfaces.push(AuthorityBoundarySurface::PublicApi);
	}
	if architecture_recovery_path_is_security(&lower) {
		surfaces.push(AuthorityBoundarySurface::Security);
	}
	if architecture_recovery_path_is_privacy(&lower) {
		surfaces.push(AuthorityBoundarySurface::Privacy);
	}
	if architecture_recovery_path_is_data(&lower) {
		surfaces.push(AuthorityBoundarySurface::Data);
	}
	if architecture_recovery_path_is_billing(&lower) {
		surfaces.push(AuthorityBoundarySurface::Billing);
	}
	if architecture_recovery_path_is_validation(&lower) {
		surfaces.push(AuthorityBoundarySurface::Validation);
	}
	if architecture_recovery_path_is_review_policy(&lower) {
		surfaces.push(AuthorityBoundarySurface::ReviewPolicy);
	}
	if surfaces.is_empty() && architecture_recovery_path_is_runtime(&lower) {
		surfaces.push(AuthorityBoundarySurface::Runtime);
	}

	surfaces
}

fn architecture_recovery_path_is_test(path: &str) -> bool {
	path.starts_with("tests/")
		|| path.contains("/tests/")
		|| path.ends_with("_test.rs")
		|| path.ends_with("tests.rs")
		|| path.contains("/test_")
}

fn architecture_recovery_path_is_config(path: &str) -> bool {
	path == "cargo.toml"
		|| path == "cargo.lock"
		|| path == "makefile.toml"
		|| path == "clippy.toml"
		|| path == "rust-toolchain.toml"
		|| path == "decodex.example.toml"
		|| path.starts_with(".github/")
		|| path.ends_with(".toml")
		|| path.ends_with(".yaml")
		|| path.ends_with(".yml")
		|| path.ends_with(".json")
		|| path.ends_with(".env")
}

fn architecture_recovery_path_is_public_api(path: &str) -> bool {
	architecture_recovery_path_has_segment(path, "cli")
		|| architecture_recovery_path_has_segment(path, "mcp")
		|| architecture_recovery_path_has_segment(path, "protocol")
		|| architecture_recovery_path_has_segment(path, "api")
		|| path.contains("tracker_tool_bridge")
		|| path.contains("app_bridge")
}

fn architecture_recovery_path_is_security(path: &str) -> bool {
	path.contains("auth")
		|| path.contains("credential")
		|| path.contains("secret")
		|| path.contains("security")
		|| path.contains("signing")
		|| path.contains("token")
}

fn architecture_recovery_path_is_privacy(path: &str) -> bool {
	path.contains("privacy") || path.contains("public_text") || path.contains("redact")
}

fn architecture_recovery_path_is_data(path: &str) -> bool {
	path.contains("database")
		|| path.contains("migration")
		|| path.contains("payload")
		|| path.contains("record")
		|| path.contains("sqlite")
		|| path.contains("state")
}

fn architecture_recovery_path_is_billing(path: &str) -> bool {
	path.contains("account")
		|| path.contains("billing")
		|| path.contains("credit")
		|| path.contains("invoice")
		|| path.contains("usage")
}

fn architecture_recovery_path_is_validation(path: &str) -> bool {
	path.contains("repo_gate")
		|| path.contains("validation")
		|| path.contains("validator")
		|| path.contains("verify")
}

fn architecture_recovery_path_is_review_policy(path: &str) -> bool {
	path.contains("review_policy") || path.contains("review_landing") || path.contains("landing")
}

fn architecture_recovery_path_is_runtime(path: &str) -> bool {
	path.starts_with("apps/") || path.starts_with("scripts/") || path.starts_with("dev/")
}

fn architecture_recovery_path_has_segment(path: &str, segment: &str) -> bool {
	path.split('/')
		.any(|part| part == segment || part.strip_suffix(".rs").is_some_and(|stem| stem == segment))
}

fn architecture_recovery_surface_summary(surface: AuthorityBoundarySurface) -> &'static str {
	match surface {
		AuthorityBoundarySurface::ImplementationStrategy =>
			"Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy.",
		AuthorityBoundarySurface::Runtime =>
			"Runtime implementation files changed during recovery.",
		AuthorityBoundarySurface::Tests => "Test files changed during recovery.",
		AuthorityBoundarySurface::Docs => "Documentation files changed during recovery.",
		AuthorityBoundarySurface::PublicApi =>
			"Public API or command surface files changed during recovery.",
		AuthorityBoundarySurface::Config => "Configuration files changed during recovery.",
		AuthorityBoundarySurface::Security =>
			"Security-sensitive implementation files changed during recovery.",
		AuthorityBoundarySurface::Data =>
			"Data or state persistence files changed during recovery.",
		AuthorityBoundarySurface::Billing => "Billing or usage files changed during recovery.",
		AuthorityBoundarySurface::Privacy => "Privacy-sensitive files changed during recovery.",
		AuthorityBoundarySurface::Validation =>
			"Validation or repository-gate files changed during recovery.",
		AuthorityBoundarySurface::ReviewPolicy =>
			"Review policy or landing policy files changed during recovery.",
		AuthorityBoundarySurface::Objective =>
			"Objective-changing recovery requires an explicit human decision.",
		AuthorityBoundarySurface::NonGoal =>
			"Non-goal-changing recovery requires an explicit human decision.",
		AuthorityBoundarySurface::ExternalDependency =>
			"External dependency recovery requires accepted authority.",
		AuthorityBoundarySurface::RetainedOwnership =>
			"Retained ownership evidence changed during recovery.",
		AuthorityBoundarySurface::AuthorityEvidence =>
			"Authority evidence changed or is insufficient during recovery.",
	}
}

pub(super) fn architecture_recovery_policy_decision(
	surfaces: &[AuthorityBoundaryChangedSurface<'_>],
) -> AuthorityBoundaryPolicyDecision {
	surfaces.iter().fold(AuthorityBoundaryPolicyDecision::AutoContinue, |decision, surface| {
		AuthorityBoundaryPolicyDecision::max(decision, surface.policy_decision)
	})
}

pub(super) fn architecture_recovery_final_reason(
	boundary: &ArchitectureRecoveryBoundary,
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	if policy_decision == boundary.policy_decision {
		return boundary.final_reason;
	}

	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => boundary.final_reason,
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence =>
			"Changed high-risk surfaces can continue recovery autonomously, but require enhanced evidence before review handoff or landing.",
		AuthorityBoundaryPolicyDecision::BlockLanding =>
			"Changed validation or review-policy surfaces can continue recovery autonomously, but block landing until the required evidence is restored.",
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => boundary.final_reason,
	}
}

pub(super) fn architecture_recovery_improvement_signals(
	reason: LoopGuardrailReason,
	boundary: &ArchitectureRecoveryBoundary,
) -> Vec<AuthorityBoundaryImprovementSignal<'static>> {
	match boundary.disposition {
		AuthorityBoundaryDisposition::WithinAuthority => match reason {
			LoopGuardrailReason::ValidationRepeat
			| LoopGuardrailReason::RemainingDeltaUnchanged => {
				vec![AuthorityBoundaryImprovementSignal {
					kind: "missing_validator",
					reason_code: "validation_guardrail_repeated",
					target: "validator:repo_gate",
					recommendation: "Promote the repeated repo-gate failure into an earlier deterministic validator or fixture.",
				}]
			},
			_ => vec![AuthorityBoundaryImprovementSignal {
				kind: "weak_prompt",
				reason_code: "architecture_recovery_strategy_needed",
				target: "prompt:phase_goal_repair",
				recommendation: "Prompt recovery agents to replace the ineffective strategy instead of repeating patch-only repair.",
			}],
		},
		AuthorityBoundaryDisposition::RequiresHuman => vec![AuthorityBoundaryImprovementSignal {
			kind: "underspecified_decision_contract",
			reason_code: "contract_boundary_required",
			target: "decision_contract:authority_envelope",
			recommendation: "Record explicit accepted authority before retrying autonomous recovery.",
		}],
		AuthorityBoundaryDisposition::InsufficientEvidence => {
			vec![AuthorityBoundaryImprovementSignal {
				kind: "underspecified_decision_contract",
				reason_code: "authority_evidence_missing",
				target: "issue_template:loop_recovery",
				recommendation: "Capture retained ownership, validation, and Decision Contract evidence before recovery.",
			}]
		},
	}
}

pub(super) fn architecture_recovery_reason_code(
	boundary: &ArchitectureRecoveryBoundary,
	policy_decision: AuthorityBoundaryPolicyDecision,
	budget_exhausted: bool,
) -> &'static str {
	if budget_exhausted {
		"architecture_recovery_exhausted"
	} else if boundary.boundary_type == AuthorityBoundarySurface::ExternalDependency {
		"external_dependency_required"
	} else if policy_decision.allows_autonomous_recovery() {
		"architecture_recovery_started"
	} else {
		"contract_boundary_required"
	}
}
