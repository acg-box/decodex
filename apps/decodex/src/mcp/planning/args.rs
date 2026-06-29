use serde::Deserialize;

use crate::research_design::{ResearchDesignOutcome, ResearchDesignRunInput};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResearchCompileToolArgs {
	pub(super) mode: Option<String>,
	pub(super) project_id: Option<String>,
	pub(super) input: Option<ResearchDesignRunInput>,
	pub(super) intent: Option<String>,
	pub(super) source_issue: Option<String>,
	pub(super) outcome: Option<ResearchDesignOutcome>,
	pub(super) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResearchPromoteToolArgs {
	pub(super) mode: Option<String>,
	pub(super) project_id: Option<String>,
	pub(super) contract_id: String,
	pub(super) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct IntakeGoalToolArgs {
	pub(super) mode: Option<String>,
	pub(super) contract_id: String,
	pub(super) team_issue_identifier: Option<String>,
	pub(super) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PlanningAuthorityArgs {
	pub(super) source: Option<String>,
	pub(super) reason: Option<String>,
	pub(super) accepted_by: Option<String>,
	pub(super) accepted_at: Option<String>,
	pub(super) acceptance_source: Option<String>,
	pub(super) run_id: Option<String>,
	pub(super) expected_turn_id: Option<String>,
}
