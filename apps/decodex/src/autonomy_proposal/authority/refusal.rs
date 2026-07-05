use crate::autonomy_proposal::AutonomyProposalRefusalReason;

impl AutonomyProposalRefusalReason {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::MissingObjective => "missing_objective",
			Self::DisallowedSignalKind => "disallowed_signal_kind",
			Self::DisallowedSurface => "disallowed_surface",
			Self::StaleEvidence => "stale_evidence",
			Self::UnresolvedContradiction => "unresolved_contradiction",
			Self::WeakenedValidationReview => "weakened_validation_review",
		}
	}
}
