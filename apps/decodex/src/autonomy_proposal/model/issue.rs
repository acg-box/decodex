use serde::{Deserialize, Serialize};

use crate::{
	autonomy_proposal::validation,
	prelude::{Result, eyre},
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalIssueCandidate {
	pub(crate) key: String,
	pub(crate) title: String,
	pub(crate) objective: String,
	pub(crate) stage: String,
	#[serde(default)]
	pub(crate) dependencies: Vec<String>,
	#[serde(default)]
	#[serde(alias = "conflictDomains")]
	pub(crate) conflict_domains: Vec<String>,
	pub(crate) acceptance: Vec<String>,
	pub(crate) validation: Vec<String>,
	#[serde(default)]
	pub(crate) risk: Vec<String>,
	#[serde(alias = "queueIntent")]
	pub(crate) queue_intent: String,
}
impl AutonomyProposalIssueCandidate {
	pub(in crate::autonomy_proposal) fn validate(&self) -> Result<()> {
		validation::validate_required("autonomy proposal issue_candidates.key", &self.key)?;
		validation::validate_required("autonomy proposal issue_candidates.title", &self.title)?;
		validation::validate_required(
			"autonomy proposal issue_candidates.objective",
			&self.objective,
		)?;
		validation::validate_required("autonomy proposal issue_candidates.stage", &self.stage)?;
		validation::validate_string_list(
			"autonomy proposal issue_candidates.dependencies",
			&self.dependencies,
		)?;
		validation::validate_string_list(
			"autonomy proposal issue_candidates.conflict_domains",
			&self.conflict_domains,
		)?;
		validation::validate_string_list(
			"autonomy proposal issue_candidates.acceptance",
			&self.acceptance,
		)?;
		validation::validate_string_list(
			"autonomy proposal issue_candidates.validation",
			&self.validation,
		)?;
		validation::validate_string_list("autonomy proposal issue_candidates.risk", &self.risk)?;
		validation::validate_required(
			"autonomy proposal issue_candidates.queue_intent",
			&self.queue_intent,
		)?;

		if self.acceptance.is_empty() {
			eyre::bail!(
				"Autonomy proposal issue candidate `{}` must include acceptance criteria.",
				self.key
			);
		}
		if self.validation.is_empty() {
			eyre::bail!(
				"Autonomy proposal issue candidate `{}` must include validation expectations.",
				self.key
			);
		}

		validation::validate_proposed_issue_stage(&self.key, &self.stage)?;

		validation::validate_proposed_issue_queue_intent(&self.key, &self.queue_intent)
	}
}
