use crate::{
	loop_contract::{DecisionContract, DecisionContractStatus, DecisionPromotion, validation},
	prelude::{Result, eyre},
};

impl DecisionContract {
	pub(crate) fn link_generated_execution_surfaces(
		&mut self,
		issue_ids: impl IntoIterator<Item = impl Into<String>>,
		issue_identifiers: impl IntoIterator<Item = impl Into<String>>,
		node_ids: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<()> {
		let mut candidate = self.clone();

		candidate.links.generated_issue_ids = validation::normalized_link_values(issue_ids)?;
		candidate.links.generated_issue_identifiers =
			validation::normalized_link_values(issue_identifiers)?;
		candidate.links.execution_program_node_ids = validation::normalized_link_values(node_ids)?;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn promote(&mut self, promotion: DecisionPromotion) -> Result<()> {
		match self.status {
			DecisionContractStatus::DraftLatent | DecisionContractStatus::NeedsHumanDecision => {},
			DecisionContractStatus::AcceptedPromoted => {
				eyre::bail!("Decision contract `{}` is already promoted.", self.contract_id);
			},
			DecisionContractStatus::RejectedSuperseded => {
				eyre::bail!(
					"Decision contract `{}` was rejected or superseded and cannot be promoted.",
					self.contract_id
				);
			},
		}

		promotion.validate()?;

		let mut candidate = self.clone();

		candidate.status = DecisionContractStatus::AcceptedPromoted;
		candidate.promotion = Some(promotion);

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn require_human_decision(&mut self, reason: impl Into<String>) -> Result<()> {
		match self.status {
			DecisionContractStatus::DraftLatent | DecisionContractStatus::NeedsHumanDecision => {},
			DecisionContractStatus::AcceptedPromoted => {
				eyre::bail!(
					"Accepted decision contract `{}` cannot be moved back to needs-human-decision.",
					self.contract_id
				);
			},
			DecisionContractStatus::RejectedSuperseded => {
				eyre::bail!(
					"Rejected decision contract `{}` cannot be moved to needs-human-decision.",
					self.contract_id
				);
			},
		}

		let reason = reason.into();

		validation::validate_required("decision contract human-decision reason", &reason)?;

		let mut candidate = self.clone();

		if !candidate
			.execution_readiness
			.missing_decisions
			.iter()
			.any(|existing| existing == &reason)
		{
			candidate.execution_readiness.missing_decisions.push(reason);
		}

		candidate.status = DecisionContractStatus::NeedsHumanDecision;
		candidate.promotion = None;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn reject_or_supersede(
		&mut self,
		superseded_by_contract_id: Option<String>,
	) -> Result<()> {
		let mut candidate = self.clone();

		if let Some(contract_id) = superseded_by_contract_id {
			validation::validate_required(
				"decision contract superseded_by_contract_id",
				&contract_id,
			)?;

			candidate.links.superseded_by_contract_id = Some(contract_id);
		}

		candidate.status = DecisionContractStatus::RejectedSuperseded;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}
}
