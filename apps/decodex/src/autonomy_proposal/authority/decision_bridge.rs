use crate::autonomy_proposal::{
	AutonomyProposalAuthorityActorKind, AutonomyProposalDecisionBridgeAuthority,
	AutonomyProposalDecisionBridgeAuthorityInput, Result, eyre, validation,
};

impl AutonomyProposalDecisionBridgeAuthority {
	pub(crate) fn new(input: AutonomyProposalDecisionBridgeAuthorityInput) -> Result<Self> {
		let authority = Self {
			accepted_by: input.accepted_by,
			accepted_by_kind: input.accepted_by_kind,
			accepted_at: input.accepted_at,
			acceptance_source: input.acceptance_source,
			reason: input.reason,
			proposal_actor: input.proposal_actor,
			proposal_actor_kind: input.proposal_actor_kind,
			accepted_project_policy: input.accepted_project_policy,
		};

		authority.validate()?;

		Ok(authority)
	}

	pub(in crate::autonomy_proposal) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"autonomy proposal acceptance.accepted_by",
			&self.accepted_by,
		)?;
		validation::validate_required(
			"autonomy proposal acceptance.accepted_at",
			&self.accepted_at,
		)?;
		validation::validate_required(
			"autonomy proposal acceptance.acceptance_source",
			&self.acceptance_source,
		)?;
		validation::validate_required("autonomy proposal acceptance.reason", &self.reason)?;
		validation::validate_required(
			"autonomy proposal acceptance.proposal_actor",
			&self.proposal_actor,
		)?;

		if let Some(policy) = &self.accepted_project_policy {
			policy.validate_for_authority(self)?;
		}

		validate_policy_backed_actor(self)?;

		validate_external_actor_self_acceptance(self)
	}
}

fn validate_policy_backed_actor(authority: &AutonomyProposalDecisionBridgeAuthority) -> Result<()> {
	if !matches!(
		authority.accepted_by_kind,
		AutonomyProposalAuthorityActorKind::RuntimePolicy
			| AutonomyProposalAuthorityActorKind::ExternalAgent
	) || authority.accepted_project_policy.is_some()
	{
		return Ok(());
	}

	eyre::bail!(
		"Autonomy proposal acceptance by `{}` requires accepted project policy authority.",
		authority.accepted_by_kind.as_str()
	);
}

fn validate_external_actor_self_acceptance(
	authority: &AutonomyProposalDecisionBridgeAuthority,
) -> Result<()> {
	if authority.proposal_actor_kind != AutonomyProposalAuthorityActorKind::ExternalAgent
		|| authority.accepted_by != authority.proposal_actor
		|| authority.accepted_project_policy.is_some()
	{
		return Ok(());
	}

	eyre::bail!(
		"External autonomy proposal actor `{}` cannot accept its own proposal without accepted project policy authority.",
		authority.accepted_by
	);
}
