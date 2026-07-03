use serde::Deserialize;
use serde_json::Value;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	mcp::{self, planning, planning::PlanningAuthorityArgs},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyDraftObjectiveToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) project_id: Option<String>,
	pub(in crate::mcp) objective: AutonomyObjectiveContract,
	pub(in crate::mcp) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyAcceptObjectiveToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) project_id: Option<String>,
	pub(in crate::mcp) objective_id: String,
	pub(in crate::mcp) objective_version: u64,
	pub(in crate::mcp) authority: Option<AutonomyObjectiveAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomyObjectiveAcceptanceArgs {
	pub(in crate::mcp) accepted_by: String,
	pub(in crate::mcp) accepted_by_kind: AutonomyObjectiveActorKind,
	pub(in crate::mcp) accepted_at: Option<String>,
	pub(in crate::mcp) acceptance_source: String,
}
impl AutonomyObjectiveAcceptanceArgs {
	pub(in crate::mcp) fn into_acceptance(self) -> Result<AutonomyObjectiveAcceptance, Value> {
		if self.accepted_by_kind == AutonomyObjectiveActorKind::RuntimePolicy {
			return Err(mcp::tool_refusal(
				"objective_acceptance_refused",
				"Runtime-policy Objective Contract acceptance must be resolved from trusted Decodex authority state; caller-supplied runtime_policy acceptance fails closed.",
			));
		}

		AutonomyObjectiveAcceptance::new(
			self.accepted_by,
			self.accepted_by_kind,
			self.accepted_at.unwrap_or_else(planning::mcp_now_rfc3339),
			self.acceptance_source,
		)
		.map_err(|error| {
			mcp::tool_refusal(
				"objective_acceptance_refused",
				format!("Objective Contract acceptance authority was refused: {error}"),
			)
		})
	}
}
