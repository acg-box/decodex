use serde::{Deserialize, Serialize};

use crate::{
	loop_contract::{schema::DecisionContractStatus, validation},
	prelude::{Result, eyre},
};

/// Natural-language readiness summary for later issue shaping.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionExecutionReadiness {
	summary: String,
	pub(super) ready_for_issue_shaping: bool,
	#[serde(default)]
	pub(super) missing_decisions: Vec<String>,
	#[serde(default)]
	validation_expectations: Vec<String>,
	#[serde(default)]
	risk_notes: Vec<String>,
	pub(super) proposed_issues: Vec<DecisionProposedIssue>,
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

	pub(super) fn validate(&self, status: DecisionContractStatus) -> Result<()> {
		validation::validate_required(
			"decision contract execution_readiness.summary",
			&self.summary,
		)?;
		validation::validate_string_list(
			"decision contract missing_decisions",
			&self.missing_decisions,
		)?;
		validation::validate_string_list(
			"decision contract validation_expectations",
			&self.validation_expectations,
		)?;
		validation::validate_string_list("decision contract risk_notes", &self.risk_notes)?;
		validation::validate_proposed_issues(&self.proposed_issues)?;
		validation::validate_string_list(
			"decision contract promotion_targets",
			&self.promotion_targets,
		)?;
		validation::validate_string_list(
			"decision contract conflict_domains",
			&self.conflict_domains,
		)?;

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
			DecisionContractStatus::NeedsHumanDecision => {
				if self.missing_decisions.is_empty() {
					eyre::bail!(
						"Needs-human-decision contracts must include at least one missing decision."
					);
				}
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

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("decision contract proposed_issues.key", &self.key)?;
		validation::validate_required("decision contract proposed_issues.title", &self.title)?;
		validation::validate_required(
			"decision contract proposed_issues.objective",
			&self.objective,
		)?;
		validation::validate_required("decision contract proposed_issues.stage", &self.stage)?;
		validation::validate_string_list(
			"decision contract proposed_issues.dependencies",
			&self.dependencies,
		)?;
		validation::validate_string_list(
			"decision contract proposed_issues.conflict_domains",
			&self.conflict_domains,
		)?;
		validation::validate_string_list(
			"decision contract proposed_issues.acceptance",
			&self.acceptance,
		)?;
		validation::validate_string_list(
			"decision contract proposed_issues.validation",
			&self.validation,
		)?;
		validation::validate_string_list("decision contract proposed_issues.risk", &self.risk)?;
		validation::validate_required(
			"decision contract proposed_issues.queue_intent",
			&self.queue_intent,
		)?;

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

		validation::validate_proposed_issue_stage(&self.key, &self.stage)?;

		validation::validate_proposed_issue_queue_intent(&self.key, &self.queue_intent)
	}
}
