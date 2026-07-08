use crate::orchestrator::execution_architecture_recovery::{
	ArchitectureRecoveryBoundary, AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision,
	AuthorityBoundarySurface, LoopGuardrailReason, LoopGuardrailStopRequested, RepoGateFailure,
	RepoGateFailureDisposition, Report,
};

pub(in crate::orchestrator::execution_architecture_recovery) fn classify_loop_guardrail_authority_boundary(
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
