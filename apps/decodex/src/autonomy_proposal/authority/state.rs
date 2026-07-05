use crate::autonomy_proposal::AutonomyProposalState;

impl AutonomyProposalState {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Draft => "draft",
			Self::NeedsEvidence => "needs_evidence",
			Self::NeedsHumanDecision => "needs_human_decision",
			Self::Rejected => "rejected",
			Self::DecisionCandidate => "decision_candidate",
			Self::AcceptedPromoted => "accepted_promoted",
		}
	}
}
