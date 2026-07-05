use crate::autonomy_proposal::AutonomyProposalAuthorityActorKind;

impl AutonomyProposalAuthorityActorKind {
	pub(in crate::autonomy_proposal) fn as_str(self) -> &'static str {
		match self {
			Self::User => "user",
			Self::RuntimePolicy => "runtime_policy",
			Self::ExternalAgent => "external_agent",
		}
	}
}
