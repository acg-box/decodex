use serde::{Deserialize, Serialize};

use crate::{
	prelude::{Result, eyre},
	research_design::normalized,
};

/// Structured issue-shaping input emitted into Decision Contract readiness.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchProposedIssueInput {
	pub(in crate::research_design) key: String,
	pub(in crate::research_design) title: String,
	pub(in crate::research_design) objective: String,
	pub(in crate::research_design) stage: String,
	pub(in crate::research_design) dependencies: Vec<String>,
	pub(in crate::research_design) conflict_domains: Vec<String>,
	pub(in crate::research_design) acceptance: Vec<String>,
	pub(in crate::research_design) validation: Vec<String>,
	pub(in crate::research_design) risk: Vec<String>,
	pub(in crate::research_design) queue_intent: String,
}
impl ResearchProposedIssueInput {
	pub(in crate::research_design) fn normalized(self) -> Result<Self> {
		let issue = Self {
			key: normalized::normalize_required_text("proposed_issues.key", self.key)?,
			title: normalized::normalize_required_text("proposed_issues.title", self.title)?,
			objective: normalized::normalize_required_text(
				"proposed_issues.objective",
				self.objective,
			)?,
			stage: normalized::normalize_required_text("proposed_issues.stage", self.stage)?,
			dependencies: normalized::normalize_text_list(
				"proposed_issues.dependencies",
				self.dependencies,
			)?,
			conflict_domains: normalized::normalize_text_list(
				"proposed_issues.conflict_domains",
				self.conflict_domains,
			)?,
			acceptance: normalized::normalize_text_list(
				"proposed_issues.acceptance",
				self.acceptance,
			)?,
			validation: normalized::normalize_text_list(
				"proposed_issues.validation",
				self.validation,
			)?,
			risk: normalized::normalize_text_list("proposed_issues.risk", self.risk)?,
			queue_intent: normalized::normalize_required_text(
				"proposed_issues.queue_intent",
				self.queue_intent,
			)?,
		};

		if issue.acceptance.is_empty() {
			eyre::bail!("proposed_issues.acceptance must include at least one item.");
		}
		if issue.validation.is_empty() {
			eyre::bail!("proposed_issues.validation must include at least one item.");
		}

		Ok(issue)
	}
}
