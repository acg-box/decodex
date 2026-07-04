use crate::{
	prelude::{Result, eyre},
	research_design::{ResearchDesignOutcome, normalized::NormalizedResearchDesignInput},
};

impl NormalizedResearchDesignInput {
	pub(in crate::research_design) fn validate_outcome(&self) -> Result<()> {
		match self.outcome {
			ResearchDesignOutcome::DecisionReady => self.validate_decision_ready(),
			ResearchDesignOutcome::NotDecisionReady => Ok(()),
			ResearchDesignOutcome::Blocked => self.validate_blocked(),
			ResearchDesignOutcome::NeedsHumanDecision => self.validate_needs_human_decision(),
		}
	}

	pub(in crate::research_design) fn ready_for_issue_shaping(&self) -> bool {
		self.outcome == ResearchDesignOutcome::DecisionReady
	}

	pub(in crate::research_design) fn missing_decisions(&self) -> Vec<String> {
		let mut missing = Vec::new();

		missing.extend(self.unresolved_decisions.clone());
		missing.extend(self.evidence_gaps.iter().map(|gap| format!("Evidence gap: {gap}")));

		if self.outcome == ResearchDesignOutcome::NotDecisionReady && missing.is_empty() {
			missing.push(String::from(
				"Research is not decision-ready; gather more evidence or narrow the decision.",
			));
		}

		missing
	}

	fn validate_decision_ready(&self) -> Result<()> {
		if self.objectives.is_empty() {
			eyre::bail!("decision-ready research requires at least one accepted objective.");
		}
		if self.evidence.is_empty() {
			eyre::bail!("decision-ready research requires at least one evidence claim.");
		}
		if self.evidence.iter().any(|evidence| evidence.kind == "unspecified") {
			eyre::bail!("decision-ready research requires an evidence kind for each claim.");
		}
		if self.options.is_empty() {
			eyre::bail!("decision-ready research requires at least one option comparison.");
		}
		if self.objections.is_empty() {
			eyre::bail!(
				"decision-ready research requires at least one recorded challenge objection or objection note."
			);
		}
		if self.validation_expectations.is_empty() {
			eyre::bail!("decision-ready research requires validation expectations.");
		}
		if self.proposed_issues.is_empty() {
			eyre::bail!(
				"decision-ready research requires at least one structured proposed issue for downstream shaping."
			);
		}
		if self.promotion_targets.is_empty() {
			eyre::bail!("decision-ready research requires at least one promotion target.");
		}
		if !self.unresolved_decisions.is_empty() || !self.evidence_gaps.is_empty() {
			eyre::bail!(
				"decision-ready research cannot carry unresolved decisions or evidence gaps."
			);
		}
		if !self.blockers.is_empty() {
			eyre::bail!("decision-ready research cannot carry unresolved blockers.");
		}

		Ok(())
	}

	fn validate_blocked(&self) -> Result<()> {
		if self.blockers.is_empty() {
			eyre::bail!("blocked research requires at least one blocker.");
		}

		Ok(())
	}

	fn validate_needs_human_decision(&self) -> Result<()> {
		if self.unresolved_decisions.is_empty() {
			eyre::bail!("needs-human-decision research requires an unresolved decision.");
		}

		Ok(())
	}
}
