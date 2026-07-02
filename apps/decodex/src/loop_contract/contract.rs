use serde::{Deserialize, Serialize};

use crate::{
	loop_contract::{
		DECISION_CONTRACT_RECORD_VERSION, DECISION_CONTRACT_SCHEMA, DecisionAcceptedAuthority,
		DecisionContractLinks, DecisionContractStatus, DecisionEvidenceBoundary,
		DecisionExecutionReadiness, DecisionPromotion, DecisionResearchEvidence,
		DecisionResearchOption, DecisionResearchProvenance, DecisionSourceIntent, validation,
	},
	prelude::{Result, eyre},
};

/// Versioned research-to-execution contract payload.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionContract {
	#[serde(default = "super::schema::decision_contract_schema")]
	pub(super) schema: String,
	#[serde(default = "super::schema::decision_contract_record_version")]
	pub(super) record_version: u16,
	pub(super) contract_id: String,
	pub(super) status: DecisionContractStatus,
	pub(super) source_intent: DecisionSourceIntent,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) research_provenance: Vec<DecisionResearchProvenance>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) research_evidence: Vec<DecisionResearchEvidence>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) research_options: Vec<DecisionResearchOption>,
	pub(super) accepted_authority: DecisionAcceptedAuthority,
	pub(super) execution_readiness: DecisionExecutionReadiness,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) promotion: Option<DecisionPromotion>,
	#[serde(default)]
	pub(super) links: DecisionContractLinks,
	pub(super) evidence_boundary: DecisionEvidenceBoundary,
}
#[allow(dead_code)]
impl DecisionContract {
	pub(crate) fn contract_id(&self) -> &str {
		&self.contract_id
	}

	pub(crate) fn status(&self) -> DecisionContractStatus {
		self.status
	}

	pub(crate) fn source_intent(&self) -> &DecisionSourceIntent {
		&self.source_intent
	}

	pub(crate) fn accepted_authority(&self) -> &DecisionAcceptedAuthority {
		&self.accepted_authority
	}

	pub(crate) fn research_options(&self) -> &[DecisionResearchOption] {
		&self.research_options
	}

	pub(crate) fn research_provenance(&self) -> &[DecisionResearchProvenance] {
		&self.research_provenance
	}

	pub(crate) fn research_evidence(&self) -> &[DecisionResearchEvidence] {
		&self.research_evidence
	}

	pub(crate) fn execution_readiness(&self) -> &DecisionExecutionReadiness {
		&self.execution_readiness
	}

	pub(crate) fn promotion(&self) -> Option<&DecisionPromotion> {
		self.promotion.as_ref()
	}

	pub(crate) fn links(&self) -> &DecisionContractLinks {
		&self.links
	}

	pub(crate) fn validate(&self) -> Result<()> {
		validation::validate_required("decision contract schema", &self.schema)?;
		validation::validate_required("decision contract contract_id", &self.contract_id)?;

		self.source_intent.validate()?;
		self.accepted_authority.validate(self.status)?;
		self.execution_readiness.validate(self.status)?;
		self.links.validate()?;
		self.evidence_boundary.validate()?;

		if self.schema != DECISION_CONTRACT_SCHEMA {
			eyre::bail!(
				"Decision contract `{}` has unsupported schema `{}`.",
				self.contract_id,
				self.schema
			);
		}
		if self.record_version != DECISION_CONTRACT_RECORD_VERSION {
			eyre::bail!(
				"Decision contract `{}` has unsupported record_version `{}`.",
				self.contract_id,
				self.record_version
			);
		}
		if self.status == DecisionContractStatus::AcceptedPromoted && self.promotion.is_none() {
			eyre::bail!(
				"Accepted decision contract `{}` must include promotion metadata.",
				self.contract_id
			);
		}
		if matches!(
			self.status,
			DecisionContractStatus::DraftLatent | DecisionContractStatus::NeedsHumanDecision
		) && self.promotion.is_some()
		{
			eyre::bail!(
				"Latent decision contract `{}` must not carry promotion metadata.",
				self.contract_id
			);
		}

		if let Some(promotion) = &self.promotion {
			promotion.validate()?;
		}

		for provenance in &self.research_provenance {
			provenance.validate()?;
		}
		for evidence in &self.research_evidence {
			evidence.validate()?;
		}
		for option in &self.research_options {
			option.validate()?;
		}

		Ok(())
	}
}
