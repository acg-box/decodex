#[allow(clippy::wildcard_imports)] use super::*;

#[allow(dead_code)]
impl AutonomyProposal {
	pub(crate) fn compile_dry_run(
		objective: Option<&AutonomyObjectiveContract>,
		signals: &[AutonomySignal],
		input: AutonomyProposalCompileInput,
	) -> Result<Self> {
		validate_compile_input(&input)?;

		for signal in signals {
			signal.validate()?;
		}

		let objective_lineage = AutonomyProposalObjectiveLineage {
			project_id: input.project_id.clone(),
			objective_id: input.objective_id.clone(),
			objective_version: input.objective_version,
			objective_state: objective.map(|objective| objective.state().as_str().to_owned()),
			objective_summary: objective.map(|objective| objective.summary().to_owned()),
		};
		let mut source_signals =
			signals.iter().map(AutonomyProposalSourceSignal::from_signal).collect::<Vec<_>>();

		source_signals.sort_by(|left, right| left.signal_id.cmp(&right.signal_id));
		source_signals.dedup_by(|left, right| left.signal_id == right.signal_id);

		let source_signal_ids =
			unique_sorted_strings(source_signals.iter().map(|signal| signal.signal_id.clone()));
		let allowed_surfaces =
			objective.map(|objective| objective.allowed_surfaces().to_vec()).unwrap_or_default();
		let validation_gates =
			objective.map(|objective| objective.validation_gates().to_vec()).unwrap_or_default();
		let goals = objective.map(|objective| objective.goals().to_vec()).unwrap_or_default();
		let metrics = objective.map(|objective| objective.metrics().to_vec()).unwrap_or_default();
		let non_goals =
			objective.map(|objective| objective.non_goals().to_vec()).unwrap_or_default();
		let review_requirements = objective
			.map(|objective| vec![objective.review_policy().to_owned()])
			.unwrap_or_default();
		let contradictions = unique_sorted_strings(
			signals.iter().flat_map(|signal| signal.contradictions().to_vec()),
		);
		let gaps = unique_sorted_strings(signals.iter().flat_map(|signal| signal.gaps().to_vec()));
		let refusal_reasons = proposal_refusals(objective, signals, &input, &contradictions);
		let state = derive_proposal_state(!source_signal_ids.is_empty(), &refusal_reasons);
		let affected_identifiers = unique_sorted_strings(input.affected_identifiers);
		let mut proposal = Self {
			schema: autonomy_proposal_schema(),
			record_version: autonomy_proposal_record_version(),
			id: String::new(),
			fingerprint: String::new(),
			project_id: input.project_id,
			objective_id: input.objective_id,
			objective_version: input.objective_version,
			state,
			source_family: input.source_family,
			intended_surface: input.intended_surface,
			affected_identifiers,
			summary: input.summary,
			objective_lineage,
			source_signal_ids,
			source_signals,
			allowed_surfaces,
			validation_gates,
			goals,
			metrics,
			non_goals,
			review_requirements,
			challenge_requirements: unique_sorted_strings(input.challenge_requirements),
			rejected_alternatives: unique_sorted_strings(input.rejected_alternatives),
			rollback_path: input.rollback_path,
			contradictions,
			gaps,
			refusal_reasons,
			challenge_evidence: Vec::new(),
			dry_run: true,
			non_executable: true,
			created_at: input.created_at,
		};
		let fingerprint = autonomy_proposal_fingerprint(&proposal)?;

		proposal.id = autonomy_proposal_id(&fingerprint);
		proposal.fingerprint = fingerprint;

		proposal.validate()?;

		Ok(proposal)
	}

	pub(crate) fn id(&self) -> &str {
		&self.id
	}

	pub(crate) fn fingerprint(&self) -> &str {
		&self.fingerprint
	}

	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn objective_id(&self) -> &str {
		&self.objective_id
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.objective_version
	}

	pub(crate) fn state(&self) -> AutonomyProposalState {
		self.state
	}

	pub(crate) fn source_family(&self) -> &str {
		&self.source_family
	}

	pub(crate) fn intended_surface(&self) -> &str {
		&self.intended_surface
	}

	pub(crate) fn affected_identifiers(&self) -> &[String] {
		&self.affected_identifiers
	}

	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn source_signal_ids(&self) -> &[String] {
		&self.source_signal_ids
	}

	pub(crate) fn allowed_surfaces(&self) -> &[String] {
		&self.allowed_surfaces
	}

	pub(crate) fn validation_gates(&self) -> &[String] {
		&self.validation_gates
	}

	pub(crate) fn contradictions(&self) -> &[String] {
		&self.contradictions
	}

	pub(crate) fn gaps(&self) -> &[String] {
		&self.gaps
	}

	pub(crate) fn refusal_reasons(&self) -> &[AutonomyProposalRefusal] {
		&self.refusal_reasons
	}

	pub(crate) fn challenge_evidence(&self) -> &[AutonomyProposalChallengeEvidence] {
		&self.challenge_evidence
	}

	pub(crate) fn has_refusal_reason(&self, reason: AutonomyProposalRefusalReason) -> bool {
		self.refusal_reasons.iter().any(|refusal| refusal.reason == reason)
	}

	pub(crate) fn record_challenge(&mut self, input: AutonomyProposalChallengeInput) -> Result<()> {
		let challenge = AutonomyProposalChallengeEvidence::from_input(input)?;
		let mut candidate = self.clone();

		candidate.challenge_evidence.push(challenge);
		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

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
				"source_issue_identifier": proposal_source_issue_identifier(&self.affected_identifiers),
			},
			"research_provenance": autonomy_decision_research_provenance(self, &authority),
			"research_evidence": autonomy_decision_research_evidence(self),
			"research_options": autonomy_decision_research_options(self),
			"accepted_authority": {
				"accepted_objectives": proposal_objectives(self),
				"non_goals": self.non_goals.clone(),
				"constraints": proposal_constraints(self),
				"assumptions": proposal_assumptions(self, &authority),
				"objections": proposal_objections(self),
				"stop_conditions": proposal_stop_conditions(self),
			},
			"execution_readiness": {
				"summary": "Accepted autonomy proposal is ready for normal Decision Contract promotion.",
				"ready_for_issue_shaping": true,
				"missing_decisions": [],
				"validation_expectations": proposal_validation_expectations(self),
				"risk_notes": proposal_risk_notes(self),
				"proposed_issues": [proposal_issue_candidate(self)],
				"promotion_targets": ["research_promote", "decision_contract"],
				"conflict_domains": proposal_conflict_domains(self),
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

	pub(crate) fn validate(&self) -> Result<()> {
		validate_required("autonomy proposal schema", &self.schema)?;
		validate_required("autonomy proposal id", &self.id)?;
		validate_required("autonomy proposal fingerprint", &self.fingerprint)?;
		validate_required("autonomy proposal project_id", &self.project_id)?;
		validate_required("autonomy proposal objective_id", &self.objective_id)?;
		validate_required("autonomy proposal source_family", &self.source_family)?;
		validate_required("autonomy proposal intended_surface", &self.intended_surface)?;
		validate_required("autonomy proposal summary", &self.summary)?;
		validate_required("autonomy proposal rollback_path", &self.rollback_path)?;
		validate_required("autonomy proposal created_at", &self.created_at)?;

		if self.schema != AUTONOMY_PROPOSAL_SCHEMA {
			eyre::bail!(
				"Autonomy proposal `{}` has unsupported schema `{}`.",
				self.id,
				self.schema
			);
		}
		if self.record_version != AUTONOMY_PROPOSAL_RECORD_VERSION {
			eyre::bail!(
				"Autonomy proposal `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.objective_version == 0 {
			eyre::bail!(
				"Autonomy proposal `{}` objective_version must be greater than zero.",
				self.id
			);
		}
		if !self.dry_run || !self.non_executable {
			eyre::bail!(
				"Autonomy proposal `{}` must remain non-executable dry-run evidence.",
				self.id
			);
		}
		if self.state == AutonomyProposalState::AcceptedPromoted {
			eyre::bail!(
				"Autonomy proposal `{}` cannot claim accepted_promoted in schema version {} without explicit Decision Contract promotion provenance.",
				self.id,
				self.record_version
			);
		}
		if self.objective_lineage.project_id != self.project_id
			|| self.objective_lineage.objective_id != self.objective_id
			|| self.objective_lineage.objective_version != self.objective_version
		{
			eyre::bail!(
				"Autonomy proposal `{}` objective lineage must match proposal key.",
				self.id
			);
		}

		self.objective_lineage.validate()?;

		validate_sorted_unique("autonomy proposal source_signal_ids", &self.source_signal_ids)?;
		validate_sorted_unique(
			"autonomy proposal affected_identifiers",
			&self.affected_identifiers,
		)?;
		validate_string_list("autonomy proposal allowed_surfaces", &self.allowed_surfaces)?;
		validate_string_list("autonomy proposal validation_gates", &self.validation_gates)?;
		validate_string_list("autonomy proposal goals", &self.goals)?;
		validate_string_list("autonomy proposal metrics", &self.metrics)?;
		validate_string_list("autonomy proposal non_goals", &self.non_goals)?;
		validate_string_list("autonomy proposal review_requirements", &self.review_requirements)?;
		validate_sorted_unique(
			"autonomy proposal challenge_requirements",
			&self.challenge_requirements,
		)?;
		validate_sorted_unique(
			"autonomy proposal rejected_alternatives",
			&self.rejected_alternatives,
		)?;
		validate_sorted_unique("autonomy proposal contradictions", &self.contradictions)?;
		validate_sorted_unique("autonomy proposal gaps", &self.gaps)?;

		let signal_ids_from_refs =
			self.source_signals.iter().map(|signal| signal.signal_id.clone()).collect::<Vec<_>>();

		if signal_ids_from_refs != self.source_signal_ids {
			eyre::bail!(
				"Autonomy proposal `{}` source_signal_ids must match source_signals.",
				self.id
			);
		}

		for signal in &self.source_signals {
			signal.validate()?;
		}
		for refusal in &self.refusal_reasons {
			refusal.validate()?;
		}
		for challenge in &self.challenge_evidence {
			challenge.validate()?;
		}

		let expected = autonomy_proposal_fingerprint(self)?;

		if expected != self.fingerprint {
			eyre::bail!(
				"Autonomy proposal `{}` fingerprint mismatch: expected `{expected}`.",
				self.id
			);
		}

		let expected_id = autonomy_proposal_id(&expected);

		if expected_id != self.id {
			eyre::bail!(
				"Autonomy proposal id `{}` does not match fingerprint `{expected}`.",
				self.id
			);
		}

		Ok(())
	}
}

impl AutonomyProposal {
	fn decision_contract_id(&self) -> String {
		format!("autonomy-decision-{}", &self.fingerprint[..32])
	}
}
