//! Versioned Loop/Decision Contract model for research-to-execution handoff.

mod evidence;
mod links;
mod promotion;
mod schema;
mod validation;

use serde::{Deserialize, Serialize};

use crate::prelude::{Result, eyre};

pub(crate) use self::{
	evidence::DecisionEvidenceBoundary,
	links::DecisionContractLinks,
	promotion::DecisionPromotion,
	schema::{
		DECISION_CONTRACT_RECORD_VERSION, DECISION_CONTRACT_SCHEMA, DecisionContractStatus,
		DecisionPromotionActorKind,
	},
};
use self::{
	schema::{decision_contract_record_version, decision_contract_schema},
	validation::{
		default_research_evidence_kind, normalized_link_values, validate_optional,
		validate_proposed_issue_queue_intent, validate_proposed_issue_stage,
		validate_proposed_issues, validate_required, validate_string_list,
	},
};

/// Versioned research-to-execution contract payload.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionContract {
	#[serde(default = "decision_contract_schema")]
	schema: String,
	#[serde(default = "decision_contract_record_version")]
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

		candidate.links.generated_issue_ids = normalized_link_values(issue_ids)?;
		candidate.links.generated_issue_identifiers = normalized_link_values(issue_identifiers)?;
		candidate.links.execution_program_node_ids = normalized_link_values(node_ids)?;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn validate(&self) -> Result<()> {
		validate_required("decision contract schema", &self.schema)?;
		validate_required("decision contract contract_id", &self.contract_id)?;

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

		validate_required("decision contract human-decision reason", &reason)?;

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
			validate_required("decision contract superseded_by_contract_id", &contract_id)?;

			candidate.links.superseded_by_contract_id = Some(contract_id);
		}

		candidate.status = DecisionContractStatus::RejectedSuperseded;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}
}

/// Natural-language source intent that led to research or design work.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionSourceIntent {
	summary: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	user_utterance: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_issue_identifier: Option<String>,
}
#[allow(dead_code)]
impl DecisionSourceIntent {
	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn source_issue_identifier(&self) -> Option<&str> {
		self.source_issue_identifier.as_deref()
	}

	fn validate(&self) -> Result<()> {
		validate_required("decision contract source_intent.summary", &self.summary)?;
		validate_optional(
			"decision contract source_intent.user_utterance",
			self.user_utterance.as_deref(),
		)?;

		validate_optional(
			"decision contract source_intent.source_issue_identifier",
			self.source_issue_identifier.as_deref(),
		)
	}
}

/// Research or design source used to produce the contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionResearchProvenance {
	kind: String,
	reference: String,
	summary: String,
}
impl DecisionResearchProvenance {
	pub(crate) fn kind(&self) -> &str {
		&self.kind
	}

	pub(crate) fn reference(&self) -> &str {
		&self.reference
	}

	fn validate(&self) -> Result<()> {
		validate_required("decision contract research_provenance.kind", &self.kind)?;
		validate_required("decision contract research_provenance.reference", &self.reference)?;

		validate_required("decision contract research_provenance.summary", &self.summary)
	}
}

/// Non-authoritative research evidence retained before promotion.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionResearchEvidence {
	#[serde(default = "default_research_evidence_kind")]
	kind: String,
	claim: String,
	support: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_ref: Option<String>,
}
impl DecisionResearchEvidence {
	pub(crate) fn kind(&self) -> &str {
		&self.kind
	}

	pub(crate) fn source_ref(&self) -> Option<&str> {
		self.source_ref.as_deref()
	}

	fn validate(&self) -> Result<()> {
		validate_required("decision contract research_evidence.kind", &self.kind)?;
		validate_required("decision contract research_evidence.claim", &self.claim)?;
		validate_required("decision contract research_evidence.support", &self.support)?;

		validate_optional(
			"decision contract research_evidence.source_ref",
			self.source_ref.as_deref(),
		)
	}
}

/// Option comparison retained as non-authoritative research context.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionResearchOption {
	option: String,
	#[serde(default)]
	tradeoffs: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	decision: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	rejected_reason: Option<String>,
}
#[allow(dead_code)]
impl DecisionResearchOption {
	pub(crate) fn option(&self) -> &str {
		&self.option
	}

	fn validate(&self) -> Result<()> {
		validate_required("decision contract research_options.option", &self.option)?;
		validate_string_list("decision contract research_options.tradeoffs", &self.tradeoffs)?;
		validate_optional("decision contract research_options.decision", self.decision.as_deref())?;

		validate_optional(
			"decision contract research_options.rejected_reason",
			self.rejected_reason.as_deref(),
		)
	}
}

/// Proposed or accepted execution authority carried by the contract.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionAcceptedAuthority {
	#[serde(default)]
	accepted_objectives: Vec<String>,
	#[serde(default)]
	non_goals: Vec<String>,
	#[serde(default)]
	constraints: Vec<String>,
	#[serde(default)]
	assumptions: Vec<String>,
	#[serde(default)]
	objections: Vec<String>,
	#[serde(default)]
	stop_conditions: Vec<String>,
}
#[allow(dead_code)]
impl DecisionAcceptedAuthority {
	pub(crate) fn accepted_objectives(&self) -> &[String] {
		&self.accepted_objectives
	}

	pub(crate) fn non_goals(&self) -> &[String] {
		&self.non_goals
	}

	pub(crate) fn constraints(&self) -> &[String] {
		&self.constraints
	}

	pub(crate) fn assumptions(&self) -> &[String] {
		&self.assumptions
	}

	pub(crate) fn objections(&self) -> &[String] {
		&self.objections
	}

	pub(crate) fn stop_conditions(&self) -> &[String] {
		&self.stop_conditions
	}

	fn validate(&self, status: DecisionContractStatus) -> Result<()> {
		if status == DecisionContractStatus::AcceptedPromoted && self.accepted_objectives.is_empty()
		{
			eyre::bail!("Accepted decision contracts must include accepted objectives.");
		}

		validate_string_list("decision contract accepted_objectives", &self.accepted_objectives)?;
		validate_string_list("decision contract non_goals", &self.non_goals)?;
		validate_string_list("decision contract constraints", &self.constraints)?;
		validate_string_list("decision contract assumptions", &self.assumptions)?;
		validate_string_list("decision contract objections", &self.objections)?;

		validate_string_list("decision contract stop_conditions", &self.stop_conditions)
	}
}

/// Natural-language readiness summary for later issue shaping.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionExecutionReadiness {
	summary: String,
	ready_for_issue_shaping: bool,
	#[serde(default)]
	missing_decisions: Vec<String>,
	#[serde(default)]
	validation_expectations: Vec<String>,
	#[serde(default)]
	risk_notes: Vec<String>,
	proposed_issues: Vec<DecisionProposedIssue>,
	#[serde(default)]
	promotion_targets: Vec<String>,
	#[serde(default)]
	conflict_domains: Vec<String>,
}
#[allow(dead_code)]
impl DecisionExecutionReadiness {
	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn ready_for_issue_shaping(&self) -> bool {
		self.ready_for_issue_shaping
	}

	pub(crate) fn missing_decisions(&self) -> &[String] {
		&self.missing_decisions
	}

	pub(crate) fn proposed_issues(&self) -> &[DecisionProposedIssue] {
		&self.proposed_issues
	}

	pub(crate) fn promotion_targets(&self) -> &[String] {
		&self.promotion_targets
	}

	pub(crate) fn conflict_domains(&self) -> &[String] {
		&self.conflict_domains
	}

	pub(crate) fn validation_expectations(&self) -> &[String] {
		&self.validation_expectations
	}

	pub(crate) fn risk_notes(&self) -> &[String] {
		&self.risk_notes
	}

	fn validate(&self, status: DecisionContractStatus) -> Result<()> {
		validate_required("decision contract execution_readiness.summary", &self.summary)?;
		validate_string_list("decision contract missing_decisions", &self.missing_decisions)?;
		validate_string_list(
			"decision contract validation_expectations",
			&self.validation_expectations,
		)?;
		validate_string_list("decision contract risk_notes", &self.risk_notes)?;
		validate_proposed_issues(&self.proposed_issues)?;
		validate_string_list("decision contract promotion_targets", &self.promotion_targets)?;
		validate_string_list("decision contract conflict_domains", &self.conflict_domains)?;

		match status {
			DecisionContractStatus::AcceptedPromoted => {
				if !self.ready_for_issue_shaping {
					eyre::bail!("Accepted decision contracts must be ready for issue shaping.");
				}
				if self.proposed_issues.is_empty() {
					eyre::bail!(
						"Accepted decision contracts must include structured proposed_issues."
					);
				}
				if !self.missing_decisions.is_empty() {
					eyre::bail!(
						"Accepted decision contracts must not carry unresolved missing decisions."
					);
				}
			},
			DecisionContractStatus::NeedsHumanDecision =>
				if self.missing_decisions.is_empty() {
					eyre::bail!(
						"Needs-human-decision contracts must include at least one missing decision."
					);
				},
			DecisionContractStatus::DraftLatent | DecisionContractStatus::RejectedSuperseded => {},
		}

		Ok(())
	}
}

/// Structured issue-shaping input retained inside Decision Contract readiness.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionProposedIssue {
	key: String,
	title: String,
	objective: String,
	stage: String,
	dependencies: Vec<String>,
	conflict_domains: Vec<String>,
	acceptance: Vec<String>,
	validation: Vec<String>,
	risk: Vec<String>,
	queue_intent: String,
}
#[allow(dead_code)]
impl DecisionProposedIssue {
	pub(crate) fn key(&self) -> &str {
		&self.key
	}

	pub(crate) fn title(&self) -> &str {
		&self.title
	}

	pub(crate) fn objective(&self) -> &str {
		&self.objective
	}

	pub(crate) fn stage(&self) -> &str {
		&self.stage
	}

	pub(crate) fn dependencies(&self) -> &[String] {
		&self.dependencies
	}

	pub(crate) fn conflict_domains(&self) -> &[String] {
		&self.conflict_domains
	}

	pub(crate) fn acceptance(&self) -> &[String] {
		&self.acceptance
	}

	pub(crate) fn validation(&self) -> &[String] {
		&self.validation
	}

	pub(crate) fn risk(&self) -> &[String] {
		&self.risk
	}

	pub(crate) fn queue_intent(&self) -> &str {
		&self.queue_intent
	}

	fn validate(&self) -> Result<()> {
		validate_required("decision contract proposed_issues.key", &self.key)?;
		validate_required("decision contract proposed_issues.title", &self.title)?;
		validate_required("decision contract proposed_issues.objective", &self.objective)?;
		validate_required("decision contract proposed_issues.stage", &self.stage)?;
		validate_string_list("decision contract proposed_issues.dependencies", &self.dependencies)?;
		validate_string_list(
			"decision contract proposed_issues.conflict_domains",
			&self.conflict_domains,
		)?;
		validate_string_list("decision contract proposed_issues.acceptance", &self.acceptance)?;
		validate_string_list("decision contract proposed_issues.validation", &self.validation)?;
		validate_string_list("decision contract proposed_issues.risk", &self.risk)?;
		validate_required("decision contract proposed_issues.queue_intent", &self.queue_intent)?;

		if self.acceptance.is_empty() {
			eyre::bail!(
				"Decision Contract proposed issue `{}` must include acceptance criteria.",
				self.key
			);
		}
		if self.validation.is_empty() {
			eyre::bail!(
				"Decision Contract proposed issue `{}` must include validation expectations.",
				self.key
			);
		}

		validate_proposed_issue_stage(&self.key, &self.stage)?;

		validate_proposed_issue_queue_intent(&self.key, &self.queue_intent)
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
