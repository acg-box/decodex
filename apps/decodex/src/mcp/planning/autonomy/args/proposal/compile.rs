use serde::Deserialize;
use serde_json::Value;

use crate::{
	autonomy_proposal::{AutonomyProposalCompileInput, AutonomyProposalIssueCandidate},
	mcp::{
		self, TOOL_AUTONOMY_COMPILE_PROPOSAL,
		planning::{self, PlanningAuthorityArgs},
	},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyCompileProposalToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) project_id: Option<String>,
	pub(in crate::mcp) proposal: AutonomyProposalCompileArgs,
	#[serde(default)]
	pub(in crate::mcp) signal_ids: Vec<String>,
	pub(in crate::mcp) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyProposalCompileArgs {
	pub(in crate::mcp) objective_id: String,
	pub(in crate::mcp) objective_version: u64,
	pub(in crate::mcp) source_family: String,
	pub(in crate::mcp) intended_surface: String,
	#[serde(default)]
	pub(in crate::mcp) affected_identifiers: Vec<String>,
	pub(in crate::mcp) summary: String,
	#[serde(default)]
	pub(in crate::mcp) challenge_requirements: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp) rejected_alternatives: Vec<String>,
	pub(in crate::mcp) rollback_path: String,
	#[serde(default)]
	pub(in crate::mcp) weakened_validation_or_review: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp) issue_candidates: Vec<AutonomyProposalIssueCandidate>,
	pub(in crate::mcp) created_at: Option<String>,
}
impl AutonomyProposalCompileArgs {
	pub(in crate::mcp) fn into_compile_input(
		self,
		project_id: &str,
	) -> Result<AutonomyProposalCompileInput, Value> {
		if self.objective_version == 0 {
			return Err(mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_COMPILE_PROPOSAL,
				"`proposal.objectiveVersion` must be greater than zero.",
			));
		}

		Ok(AutonomyProposalCompileInput {
			project_id: project_id.to_owned(),
			objective_id: self.objective_id,
			objective_version: self.objective_version,
			source_family: self.source_family,
			intended_surface: self.intended_surface,
			affected_identifiers: self.affected_identifiers,
			summary: self.summary,
			challenge_requirements: self.challenge_requirements,
			rejected_alternatives: self.rejected_alternatives,
			rollback_path: self.rollback_path,
			weakened_validation_or_review: self.weakened_validation_or_review,
			issue_candidates: self.issue_candidates,
			created_at: self.created_at.unwrap_or_else(planning::mcp_now_rfc3339),
		})
	}
}
