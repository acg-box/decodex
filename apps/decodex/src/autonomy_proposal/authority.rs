#[allow(clippy::wildcard_imports)] use super::*;

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
	#[allow(clippy::too_many_arguments)]
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn new(
		project_id: impl Into<String>,
		objective_id: impl Into<String>,
		objective_version: u64,
		accepted_policy_id: impl Into<String>,
		accepted_policy_version: impl Into<String>,
		authority_ref: impl Into<String>,
		authorized_actor: impl Into<String>,
		authorized_actor_kind: AutonomyProposalAuthorityActorKind,
		authorized_acceptance_sources: Vec<String>,
		authorized_scopes: Vec<String>,
	) -> Result<Self> {
		let policy = Self {
			project_id: project_id.into(),
			objective_id: objective_id.into(),
			objective_version,
			accepted_policy_id: accepted_policy_id.into(),
			accepted_policy_version: accepted_policy_version.into(),
			authority_ref: authority_ref.into(),
			authorized_actor: authorized_actor.into(),
			authorized_actor_kind,
			authorized_acceptance_sources,
			authorized_scopes,
		};

		policy.validate()?;

		Ok(policy)
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required(
			"autonomy proposal accepted project policy.project_id",
			&self.project_id,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.objective_id",
			&self.objective_id,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.accepted_policy_id",
			&self.accepted_policy_id,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.accepted_policy_version",
			&self.accepted_policy_version,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.authority_ref",
			&self.authority_ref,
		)?;
		validate_required(
			"autonomy proposal accepted project policy.authorized_actor",
			&self.authorized_actor,
		)?;
		validate_string_list(
			"autonomy proposal accepted project policy.authorized_acceptance_sources",
			&self.authorized_acceptance_sources,
		)?;
		validate_string_list(
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
	#[allow(clippy::too_many_arguments)]
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn new(
		accepted_by: impl Into<String>,
		accepted_by_kind: AutonomyProposalAuthorityActorKind,
		accepted_at: impl Into<String>,
		acceptance_source: impl Into<String>,
		reason: impl Into<String>,
		proposal_actor: impl Into<String>,
		proposal_actor_kind: AutonomyProposalAuthorityActorKind,
		accepted_project_policy: Option<AutonomyProposalAcceptedProjectPolicy>,
	) -> Result<Self> {
		let authority = Self {
			accepted_by: accepted_by.into(),
			accepted_by_kind,
			accepted_at: accepted_at.into(),
			acceptance_source: acceptance_source.into(),
			reason: reason.into(),
			proposal_actor: proposal_actor.into(),
			proposal_actor_kind,
			accepted_project_policy,
		};

		authority.validate()?;

		Ok(authority)
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required("autonomy proposal acceptance.accepted_by", &self.accepted_by)?;
		validate_required("autonomy proposal acceptance.accepted_at", &self.accepted_at)?;
		validate_required(
			"autonomy proposal acceptance.acceptance_source",
			&self.acceptance_source,
		)?;
		validate_required("autonomy proposal acceptance.reason", &self.reason)?;
		validate_required("autonomy proposal acceptance.proposal_actor", &self.proposal_actor)?;

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
