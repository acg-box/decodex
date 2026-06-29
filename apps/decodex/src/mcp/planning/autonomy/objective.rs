use serde_json::{self, Value};

use crate::{
	autonomy_objective::AutonomyObjectiveState,
	mcp::{
		McpServer, TOOL_AUTONOMY_ACCEPT_OBJECTIVE, TOOL_AUTONOMY_DRAFT_OBJECTIVE,
		invalid_tool_arguments, non_empty_string, safe_autonomy_record_identifier, tool_refusal,
		tool_success,
	},
};

use super::{
	super::{
		missing_authority_refusal, planning_authority_present, planning_mode, planning_project_id,
		planning_state_store,
	},
	args::{AutonomyAcceptObjectiveToolArgs, AutonomyDraftObjectiveToolArgs},
	results::{autonomy_objective_accept_tool_result, autonomy_objective_tool_result},
};

impl McpServer {
	pub(in crate::mcp) fn call_autonomy_draft_objective_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyDraftObjectiveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_DRAFT_OBJECTIVE,
					"`objective` is required and `mode` must be dry_run or apply.",
				),
		};
		let mode =
			match planning_mode(params.mode.as_deref(), "dry_run", TOOL_AUTONOMY_DRAFT_OBJECTIVE) {
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_DRAFT_OBJECTIVE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};

		if params.objective.project_id() != project_id {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_DRAFT_OBJECTIVE,
				"`objective.project_id` must match the MCP project context.",
			);
		}
		if params.objective.state() != AutonomyObjectiveState::Draft {
			return tool_refusal(
				"objective_draft_refused",
				"autonomy_draft_objective only stores draft Objective Contracts; acceptance uses a separate explicit authority surface.",
			);
		}

		if let Err(error) = params.objective.validate() {
			return tool_refusal(
				"objective_draft_refused",
				format!("Objective Contract draft did not validate: {error}"),
			);
		}

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_DRAFT_OBJECTIVE,
				"autonomy_draft_objective apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			return tool_success(autonomy_objective_tool_result(
				&project_id,
				&params.objective,
				mode,
				false,
				None,
			));
		}

		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_DRAFT_OBJECTIVE) {
			Ok(store) => store,
			Err(result) => return result,
		};

		match store.upsert_autonomy_objective_draft(&project_id, params.objective) {
			Ok(record) => tool_success(autonomy_objective_tool_result(
				&project_id,
				record.objective(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"objective_draft_refused",
				format!(
					"Objective Contract draft was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	pub(in crate::mcp) fn call_autonomy_accept_objective_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyAcceptObjectiveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
					"`objectiveId`, `objectiveVersion`, and optional `mode` are required.",
				),
		};
		let Some(objective_id) = non_empty_string(Some(params.objective_id.as_str())) else {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveId` is required.",
			);
		};

		if !safe_autonomy_record_identifier(objective_id) {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveId` must be a safe Decodex autonomy identifier.",
			);
		}
		if params.objective_version == 0 {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveVersion` must be greater than zero.",
			);
		}

		let mode = match planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_ACCEPT_OBJECTIVE) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let record =
			match store.autonomy_objective(&project_id, objective_id, params.objective_version) {
				Ok(Some(record)) => record,
				Ok(None) =>
					return tool_refusal(
						"objective_not_found",
						"Autonomy Objective Contract draft was not found in the current Decodex project.",
					),
				Err(error) =>
					return tool_refusal(
						"objective_acceptance_refused",
						format!("Objective Contract readback failed closed: {error}"),
					),
			};

		if record.state() != AutonomyObjectiveState::Draft {
			return tool_refusal(
				"objective_acceptance_refused",
				"Only draft Objective Contract versions can be accepted through autonomy_accept_objective.",
			);
		}
		if mode == "dry_run" {
			return tool_success(autonomy_objective_accept_tool_result(
				&project_id,
				record.objective(),
				mode,
				false,
				Some(record.updated_at()),
			));
		}

		let Some(authority) = params.authority else {
			return missing_authority_refusal(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"autonomy_accept_objective apply requires explicit objective acceptance authority.",
			);
		};
		let acceptance = match authority.into_acceptance() {
			Ok(acceptance) => acceptance,
			Err(result) => return result,
		};

		match store.accept_autonomy_objective_version(
			&project_id,
			objective_id,
			params.objective_version,
			acceptance,
		) {
			Ok(record) => tool_success(autonomy_objective_accept_tool_result(
				&project_id,
				record.objective(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"objective_acceptance_refused",
				format!(
					"Objective Contract acceptance was refused by Decodex authority checks: {error}"
				),
			),
		}
	}
}
