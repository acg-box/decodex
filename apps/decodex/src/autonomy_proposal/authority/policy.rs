use crate::autonomy_proposal::{
	AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE, AutonomyProposal, AutonomyProposalAcceptedProjectPolicy,
	AutonomyProposalDecisionBridgeAuthority, Result, eyre, validation,
};

impl AutonomyProposalAcceptedProjectPolicy {
	pub(in crate::autonomy_proposal) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"autonomy proposal accepted project policy.project_id",
			&self.project_id,
		)?;
		validation::validate_required(
			"autonomy proposal accepted project policy.objective_id",
			&self.objective_id,
		)?;
		validation::validate_required(
			"autonomy proposal accepted project policy.accepted_policy_id",
			&self.accepted_policy_id,
		)?;
		validation::validate_required(
			"autonomy proposal accepted project policy.accepted_policy_version",
			&self.accepted_policy_version,
		)?;
		validation::validate_required(
			"autonomy proposal accepted project policy.authority_ref",
			&self.authority_ref,
		)?;
		validation::validate_required(
			"autonomy proposal accepted project policy.authorized_actor",
			&self.authorized_actor,
		)?;
		validation::validate_string_list(
			"autonomy proposal accepted project policy.authorized_acceptance_sources",
			&self.authorized_acceptance_sources,
		)?;
		validation::validate_string_list(
			"autonomy proposal accepted project policy.authorized_scopes",
			&self.authorized_scopes,
		)?;

		validate_objective_version(self)?;
		validate_acceptance_scope(self)?;

		Ok(())
	}

	pub(in crate::autonomy_proposal) fn validate_for_authority(
		&self,
		authority: &AutonomyProposalDecisionBridgeAuthority,
	) -> Result<()> {
		self.validate()?;

		validate_authorized_actor(self, authority)?;
		validate_acceptance_source(self, authority)?;

		Ok(())
	}

	pub(in crate::autonomy_proposal) fn validate_for_proposal(
		&self,
		proposal: &AutonomyProposal,
		authority: &AutonomyProposalDecisionBridgeAuthority,
	) -> Result<()> {
		self.validate_for_authority(authority)?;

		if self.project_id != proposal.project_id
			|| self.objective_id != proposal.objective_id
			|| self.objective_version != proposal.objective_version
		{
			eyre::bail!(
				"Autonomy proposal accepted project policy `{}` does not match proposal `{}` objective lineage.",
				self.authority_ref,
				proposal.id
			);
		}

		Ok(())
	}
}

fn validate_objective_version(policy: &AutonomyProposalAcceptedProjectPolicy) -> Result<()> {
	if policy.objective_version > 0 {
		return Ok(());
	}

	eyre::bail!(
		"Autonomy proposal accepted project policy objective_version must be greater than zero."
	);
}

fn validate_acceptance_scope(policy: &AutonomyProposalAcceptedProjectPolicy) -> Result<()> {
	if policy.authorized_scopes.iter().any(|scope| scope == AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE) {
		return Ok(());
	}

	eyre::bail!(
		"Autonomy proposal accepted project policy `{}` must authorize `{}` scope.",
		policy.authority_ref,
		AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE
	);
}

fn validate_authorized_actor(
	policy: &AutonomyProposalAcceptedProjectPolicy,
	authority: &AutonomyProposalDecisionBridgeAuthority,
) -> Result<()> {
	if policy.authorized_actor == authority.accepted_by
		&& policy.authorized_actor_kind == authority.accepted_by_kind
	{
		return Ok(());
	}

	eyre::bail!(
		"Autonomy proposal accepted project policy `{}` authorizes `{}` ({}) but acceptance is by `{}` ({}).",
		policy.authority_ref,
		policy.authorized_actor,
		policy.authorized_actor_kind.as_str(),
		authority.accepted_by,
		authority.accepted_by_kind.as_str()
	);
}

fn validate_acceptance_source(
	policy: &AutonomyProposalAcceptedProjectPolicy,
	authority: &AutonomyProposalDecisionBridgeAuthority,
) -> Result<()> {
	if policy
		.authorized_acceptance_sources
		.iter()
		.any(|source| source == &authority.acceptance_source)
	{
		return Ok(());
	}

	eyre::bail!(
		"Autonomy proposal accepted project policy `{}` does not authorize acceptance source `{}`.",
		policy.authority_ref,
		authority.acceptance_source
	);
}
