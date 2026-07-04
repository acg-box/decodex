use crate::autonomy_proposal::{
	AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE, AutonomyProposal, AutonomyProposalAcceptedProjectPolicy,
	AutonomyProposalAuthorityActorKind, AutonomyProposalDecisionBridgeAuthority,
	AutonomyProposalDecisionBridgeAuthorityInput, AutonomyProposalRefusalReason,
	AutonomyProposalState, Result, eyre,
	validation::{self},
};

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

impl AutonomyProposalAuthorityActorKind {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::User => "user",
			Self::RuntimePolicy => "runtime_policy",
			Self::ExternalAgent => "external_agent",
		}
	}
}

impl AutonomyProposalAcceptedProjectPolicy {
	pub(super) fn validate(&self) -> Result<()> {
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

		if self.objective_version == 0 {
			eyre::bail!(
				"Autonomy proposal accepted project policy objective_version must be greater than zero."
			);
		}
		if !self.authorized_scopes.iter().any(|scope| scope == AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE) {
			eyre::bail!(
				"Autonomy proposal accepted project policy `{}` must authorize `{}` scope.",
				self.authority_ref,
				AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE
			);
		}

		Ok(())
	}

	fn validate_for_authority(
		&self,
		authority: &AutonomyProposalDecisionBridgeAuthority,
	) -> Result<()> {
		self.validate()?;

		if self.authorized_actor != authority.accepted_by
			|| self.authorized_actor_kind != authority.accepted_by_kind
		{
			eyre::bail!(
				"Autonomy proposal accepted project policy `{}` authorizes `{}` ({}) but acceptance is by `{}` ({}).",
				self.authority_ref,
				self.authorized_actor,
				self.authorized_actor_kind.as_str(),
				authority.accepted_by,
				authority.accepted_by_kind.as_str()
			);
		}
		if !self
			.authorized_acceptance_sources
			.iter()
			.any(|source| source == &authority.acceptance_source)
		{
			eyre::bail!(
				"Autonomy proposal accepted project policy `{}` does not authorize acceptance source `{}`.",
				self.authority_ref,
				authority.acceptance_source
			);
		}

		Ok(())
	}

	pub(super) fn validate_for_proposal(
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

	pub(super) fn validate(&self) -> Result<()> {
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

		if matches!(
			self.accepted_by_kind,
			AutonomyProposalAuthorityActorKind::RuntimePolicy
				| AutonomyProposalAuthorityActorKind::ExternalAgent
		) && self.accepted_project_policy.is_none()
		{
			eyre::bail!(
				"Autonomy proposal acceptance by `{}` requires accepted project policy authority.",
				self.accepted_by_kind.as_str()
			);
		}
		if self.proposal_actor_kind == AutonomyProposalAuthorityActorKind::ExternalAgent
			&& self.accepted_by == self.proposal_actor
			&& self.accepted_project_policy.is_none()
		{
			eyre::bail!(
				"External autonomy proposal actor `{}` cannot accept its own proposal without accepted project policy authority.",
				self.accepted_by
			);
		}

		Ok(())
	}
}
