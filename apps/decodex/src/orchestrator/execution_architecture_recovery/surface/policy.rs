use crate::orchestrator::execution_architecture_recovery::{
	ArchitectureRecoveryBoundary, AuthorityBoundaryChangedSurface, AuthorityBoundaryDisposition,
	AuthorityBoundaryImprovementSignal, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	LoopGuardrailReason,
};

pub(in crate::orchestrator::execution_architecture_recovery) fn architecture_recovery_policy_decision(
	surfaces: &[AuthorityBoundaryChangedSurface<'_>],
) -> AuthorityBoundaryPolicyDecision {
	surfaces.iter().fold(AuthorityBoundaryPolicyDecision::AutoContinue, |decision, surface| {
		AuthorityBoundaryPolicyDecision::max(decision, surface.policy_decision)
	})
}

pub(in crate::orchestrator::execution_architecture_recovery) fn architecture_recovery_final_reason(
	boundary: &ArchitectureRecoveryBoundary,
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	if policy_decision == boundary.policy_decision {
		return boundary.final_reason;
	}

	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => boundary.final_reason,
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence => {
			"Changed high-risk surfaces can continue recovery autonomously, but require enhanced evidence before review handoff or landing."
		},
		AuthorityBoundaryPolicyDecision::BlockLanding => {
			"Changed validation or review-policy surfaces can continue recovery autonomously, but block landing until the required evidence is restored."
		},
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => boundary.final_reason,
	}
}

pub(in crate::orchestrator::execution_architecture_recovery) fn architecture_recovery_improvement_signals(
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

pub(in crate::orchestrator::execution_architecture_recovery) fn architecture_recovery_reason_code(
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
