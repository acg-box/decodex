//! Versioned Loop/Decision Contract model for research-to-execution handoff.

pub(crate) mod authority;
pub(crate) mod evidence;
pub(crate) mod links;
pub(crate) mod promotion;
pub(crate) mod readiness;
pub(crate) mod research;
pub(crate) mod schema;
pub(crate) mod source_intent;
pub(crate) mod validation;

pub(crate) use self::{
	authority::DecisionAcceptedAuthority,
	evidence::DecisionEvidenceBoundary,
	links::DecisionContractLinks,
	promotion::DecisionPromotion,
	readiness::{DecisionExecutionReadiness, DecisionProposedIssue},
	research::{DecisionResearchEvidence, DecisionResearchOption, DecisionResearchProvenance},
	schema::{
		DECISION_CONTRACT_RECORD_VERSION, DECISION_CONTRACT_SCHEMA, DecisionContractStatus,
		DecisionPromotionActorKind,
	},
	source_intent::DecisionSourceIntent,
};

use serde::{Deserialize, Serialize};

use crate::prelude::{Result, eyre};

/// Versioned research-to-execution contract payload.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionContract {
	#[serde(default = "schema::decision_contract_schema")]
	schema: String,
	#[serde(default = "schema::decision_contract_record_version")]
	record_version: u16,
	contract_id: String,
	status: DecisionContractStatus,
	source_intent: DecisionSourceIntent,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	research_provenance: Vec<DecisionResearchProvenance>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	research_evidence: Vec<DecisionResearchEvidence>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	research_options: Vec<DecisionResearchOption>,
	accepted_authority: DecisionAcceptedAuthority,
	execution_readiness: DecisionExecutionReadiness,
	#[serde(skip_serializing_if = "Option::is_none")]
	promotion: Option<DecisionPromotion>,
	#[serde(default)]
	links: DecisionContractLinks,
	evidence_boundary: DecisionEvidenceBoundary,
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

#[cfg(test)]
mod tests {
	use crate::loop_contract::{
		DecisionContract, DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind,
	};

	fn latent_research_contract_fixture() -> DecisionContract {
		serde_json::from_str(include_str!(concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/fixtures/decision_contract/research_x_latent_contract.json"
		)))
		.expect("research X latent contract fixture should deserialize")
	}

	fn sample_promotion() -> DecisionPromotion {
		DecisionPromotion {
			accepted_by: String::from("operator"),
			accepted_by_kind: DecisionPromotionActorKind::User,
			accepted_at: String::from("2026-06-09T10:00:00Z"),
			acceptance_source: String::from("conversation"),
			promotion_reason: Some(String::from("User asked to push this forward.")),
		}
	}

	#[test]
	fn latent_research_contract_fixture_serializes_with_expected_boundary() {
		let contract = latent_research_contract_fixture();

		contract.validate().expect("latent contract should validate");

		assert_eq!(contract.contract_id(), "research-x-loop-contract");
		assert_eq!(contract.status(), DecisionContractStatus::DraftLatent);
		assert_eq!(contract.source_intent().summary(), "Research X and shape follow-up work.");
		assert_eq!(contract.accepted_authority().accepted_objectives().len(), 2);
		assert!(contract.execution_readiness().ready_for_issue_shaping());
		assert_eq!(contract.evidence_boundary.private_evidence_refs().len(), 1);
		assert_eq!(contract.evidence_boundary.public_projection_refs().len(), 1);
		assert!(contract.promotion().is_none());
	}

	#[test]
	fn promotion_records_acceptance_metadata_and_blocks_double_promotion() {
		let mut contract = latent_research_contract_fixture();

		contract.promote(sample_promotion()).expect("latent contract should promote");

		assert_eq!(contract.status(), DecisionContractStatus::AcceptedPromoted);
		assert_eq!(
			contract.promotion().expect("promotion should exist").accepted_at(),
			"2026-06-09T10:00:00Z"
		);
		assert!(
			contract
				.promote(contract.promotion().expect("promotion should exist").clone())
				.is_err()
		);
	}

	#[test]
	fn rejected_contract_cannot_be_promoted() {
		let mut contract = latent_research_contract_fixture();

		contract
			.reject_or_supersede(Some(String::from("research-x-replacement")))
			.expect("contract should reject");

		assert_eq!(contract.status(), DecisionContractStatus::RejectedSuperseded);
		assert_eq!(contract.links().superseded_by_contract_id(), Some("research-x-replacement"));
		assert!(
			contract
				.promote(DecisionPromotion { promotion_reason: None, ..sample_promotion() })
				.is_err()
		);
	}

	#[test]
	fn accepted_contracts_require_readiness_without_missing_decisions() {
		let mut contract = latent_research_contract_fixture();

		contract.execution_readiness.ready_for_issue_shaping = false;

		let before_failed_promotion = contract.clone();

		assert!(contract.promote(sample_promotion()).is_err());
		assert_eq!(
			contract, before_failed_promotion,
			"failed promotion must not mutate the contract"
		);

		let mut contract = latent_research_contract_fixture();

		contract
			.execution_readiness
			.missing_decisions
			.push(String::from("Choose the first generated issue."));

		let before_failed_promotion = contract.clone();

		assert!(contract.promote(sample_promotion()).is_err());
		assert_eq!(
			contract, before_failed_promotion,
			"failed promotion must not mutate the contract"
		);

		let mut contract = latent_research_contract_fixture();

		contract.execution_readiness.proposed_issues.clear();

		let before_failed_promotion = contract.clone();

		assert!(contract.promote(sample_promotion()).is_err());
		assert_eq!(
			contract, before_failed_promotion,
			"failed promotion must not mutate the contract"
		);
	}

	#[test]
	fn proposed_issue_dependencies_must_form_known_acyclic_dag() {
		let mut missing_dependency = serde_json::to_value(latent_research_contract_fixture())
			.expect("fixture should encode");

		missing_dependency["execution_readiness"]["proposed_issues"][0]["dependencies"] =
			serde_json::json!(["missing-node"]);

		let error = serde_json::from_value::<DecisionContract>(missing_dependency)
			.expect("contract should deserialize")
			.validate()
			.expect_err("unknown dependency must be rejected");

		assert!(error.to_string().contains("depends on unknown issue `missing-node`"));

		let self_key = latent_research_contract_fixture().execution_readiness().proposed_issues()
			[0]
		.key()
		.to_owned();
		let mut self_dependency = serde_json::to_value(latent_research_contract_fixture())
			.expect("fixture should encode");

		self_dependency["execution_readiness"]["proposed_issues"][0]["dependencies"] =
			serde_json::json!([self_key]);

		let error = serde_json::from_value::<DecisionContract>(self_dependency)
			.expect("contract should deserialize")
			.validate()
			.expect_err("self dependency must be rejected");

		assert!(error.to_string().contains("must not depend on itself"));

		let mut cyclic = serde_json::to_value(latent_research_contract_fixture())
			.expect("fixture should encode");
		let first_key = cyclic["execution_readiness"]["proposed_issues"][0]["key"]
			.as_str()
			.expect("first key")
			.to_owned();
		let mut second = cyclic["execution_readiness"]["proposed_issues"][0].clone();

		second["key"] = serde_json::json!("research-x-validation");
		second["title"] = serde_json::json!("Validate the Decision Contract graph.");
		second["objective"] = serde_json::json!("Validate the Decision Contract graph.");
		second["dependencies"] = serde_json::json!([first_key]);
		cyclic["execution_readiness"]["proposed_issues"][0]["dependencies"] =
			serde_json::json!(["research-x-validation"]);
		cyclic["execution_readiness"]["proposed_issues"]
			.as_array_mut()
			.expect("proposed issues should be array")
			.push(second);

		let error = serde_json::from_value::<DecisionContract>(cyclic)
			.expect("contract should deserialize")
			.validate()
			.expect_err("dependency cycle must be rejected");

		assert!(error.to_string().contains("dependency cycle includes"));
	}

	#[test]
	fn removed_flat_proposed_issue_summaries_are_rejected() {
		let mut payload = serde_json::to_value(latent_research_contract_fixture())
			.expect("fixture should encode");
		let readiness = payload
			.get_mut("execution_readiness")
			.expect("readiness should exist")
			.as_object_mut()
			.expect("readiness should be an object");

		readiness.remove("proposed_issues");
		readiness.insert(
			String::from("proposed_issue_summaries"),
			serde_json::json!(["Removed flat summary."]),
		);

		let error = serde_json::from_value::<DecisionContract>(payload)
			.expect_err("removed flat summaries must not deserialize");

		assert!(error.to_string().contains("proposed_issue_summaries"));
	}

	#[test]
	fn latent_contracts_reject_promotion_metadata() {
		let mut contract = latent_research_contract_fixture();

		contract.promotion = Some(sample_promotion());

		assert!(contract.validate().is_err());
	}

	#[test]
	fn validation_rejects_empty_optional_boundary_values() {
		let mut contract = latent_research_contract_fixture();

		contract.links.generated_issue_identifiers.push(String::from(" "));

		assert!(contract.validate().is_err());

		let mut contract = latent_research_contract_fixture();

		contract.evidence_boundary.public_summary = Some(String::new());

		assert!(contract.validate().is_err());

		let mut contract = latent_research_contract_fixture();

		contract.evidence_boundary.private_evidence_refs[0].record_id = Some(0);

		assert!(contract.validate().is_err());
	}

	#[test]
	fn generated_execution_links_are_normalized_and_validated() {
		let mut contract = latent_research_contract_fixture();

		contract
			.link_generated_execution_surfaces(
				[" issue-1 ", "issue-1", "issue-2"],
				["XY-1", " XY-2 "],
				["node-1", "node-1"],
			)
			.expect("links should attach");

		assert_eq!(contract.links().generated_issue_ids(), &["issue-1", "issue-2"]);
		assert_eq!(contract.links().generated_issue_identifiers(), &["XY-1", "XY-2"]);
		assert_eq!(contract.links().execution_program_node_ids(), &["node-1"]);
		assert!(contract.link_generated_execution_surfaces([" "], ["XY-1"], ["node-1"]).is_err());
	}

	#[test]
	fn failed_non_promotion_transitions_leave_contract_unchanged() {
		let mut contract = latent_research_contract_fixture();
		let before_failed_human_decision = contract.clone();

		assert!(contract.require_human_decision(" ").is_err());
		assert_eq!(
			contract, before_failed_human_decision,
			"failed human-decision transition must not mutate the contract"
		);

		let before_failed_rejection = contract.clone();

		assert!(contract.reject_or_supersede(Some(String::from(" "))).is_err());
		assert_eq!(
			contract, before_failed_rejection,
			"failed rejection transition must not mutate the contract"
		);
	}
}
