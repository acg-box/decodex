use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalDecisionBridgeAuthority, AutonomyProposalState, decision,
	},
	loop_contract::{DecisionContract, DecisionContractStatus},
	prelude::{Result, eyre},
};

#[allow(dead_code)]
impl AutonomyProposal {
	pub(crate) fn to_decision_contract_candidate(
		&self,
		authority: AutonomyProposalDecisionBridgeAuthority,
	) -> Result<DecisionContract> {
		self.validate()?;
		authority.validate()?;

		if let Some(policy) = &authority.accepted_project_policy {
			policy.validate_for_proposal(self, &authority)?;
		}

		if self.state != AutonomyProposalState::DecisionCandidate {
			eyre::bail!(
				"Autonomy proposal `{}` is `{}` and cannot become a Decision Contract candidate.",
				self.id,
				self.state.as_str()
			);
		}
		if !self.refusal_reasons.is_empty() {
			eyre::bail!(
				"Autonomy proposal `{}` has refusal reasons and cannot become a Decision Contract candidate.",
				self.id
			);
		}

		let payload = serde_json::json!({
			"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
			"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
			"contract_id": self.decision_contract_id(),
			"status": DecisionContractStatus::DraftLatent.as_str(),
			"source_intent": {
				"summary": format!("Accepted autonomy proposal: {}", self.summary),
				"user_utterance": authority.reason.clone(),
				"source_issue_identifier": decision::proposal_source_issue_identifier(&self.affected_identifiers),
			},
			"research_provenance": decision::autonomy_decision_research_provenance(self, &authority),
			"research_evidence": decision::autonomy_decision_research_evidence(self),
			"research_options": decision::autonomy_decision_research_options(self),
			"accepted_authority": {
				"accepted_objectives": decision::proposal_objectives(self),
				"non_goals": self.non_goals.clone(),
				"constraints": decision::proposal_constraints(self),
				"assumptions": decision::proposal_assumptions(self, &authority),
				"objections": decision::proposal_objections(self),
				"stop_conditions": decision::proposal_stop_conditions(self),
			},
			"execution_readiness": {
				"summary": "Accepted autonomy proposal is ready for normal Decision Contract promotion.",
				"ready_for_issue_shaping": true,
				"missing_decisions": [],
				"validation_expectations": decision::proposal_validation_expectations(self),
				"risk_notes": decision::proposal_risk_notes(self),
				"proposed_issues": decision::proposal_issue_candidates(self),
				"promotion_targets": ["accepted_decision_contract"],
				"conflict_domains": decision::proposal_conflict_domains(self),
			},
			"links": {
				"generated_issue_ids": [],
				"generated_issue_identifiers": [],
				"execution_program_node_ids": [],
			},
			"evidence_boundary": {
				"private_evidence_refs": [],
				"public_projection_refs": [
					{
						"surface": "autonomy_proposal",
						"reference": self.id.clone(),
						"summary": "Accepted autonomy proposal converted to latent Decision Contract candidate."
					}
				],
				"public_summary": "Autonomy proposal preserved as a latent Decision Contract candidate."
			},
		});
		let contract = serde_json::from_value::<DecisionContract>(payload)?;

		contract.validate()?;

		Ok(contract)
	}

	fn decision_contract_id(&self) -> String {
		format!("autonomy-decision-{}", &self.fingerprint[..32])
	}
}
