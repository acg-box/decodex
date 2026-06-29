use serde_json::Value;

use crate::{
	config::ServiceConfig,
	mcp::{
		self, McpServer, TOOL_INTAKE_GOAL,
		planning::{self, args::IntakeGoalToolArgs, results, tracker::McpDryRunTracker},
	},
	program_intake::{self, GoalIntakeCommandRequest, GoalIntakeRunRequest},
	workflow::WorkflowDocument,
};

impl McpServer {
	pub(in crate::mcp) fn call_intake_goal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<IntakeGoalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_INTAKE_GOAL,
					"`contractId` is required and `mode` must be dry_run or apply.",
				);
			},
		};
		let Some(contract_id) = mcp::non_empty_string(Some(params.contract_id.as_str())) else {
			return mcp::invalid_tool_arguments(TOOL_INTAKE_GOAL, "`contractId` is required.");
		};

		if !mcp::safe_runtime_identifier(contract_id) {
			return mcp::invalid_tool_arguments(
				TOOL_INTAKE_GOAL,
				"`contractId` must be a safe Decodex runtime identifier.",
			);
		}

		let mode =
			match planning::planning_mode(params.mode.as_deref(), "dry_run", TOOL_INTAKE_GOAL) {
				Ok(mode) => mode,
				Err(result) => return result,
			};

		if mode == "apply" {
			if !planning::planning_authority_present(params.authority.as_ref()) {
				return planning::missing_authority_refusal(
					TOOL_INTAKE_GOAL,
					"intake_goal apply requires authority.source and authority.reason.",
				);
			}

			return self
				.apply_intake_goal_tool(contract_id, params.team_issue_identifier.as_deref());
		}

		let store = match planning::planning_state_store(&self.context, TOOL_INTAKE_GOAL) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let config_path = match self.context.config_path.as_deref() {
			Some(path) => path,
			None => {
				return mcp::tool_refusal(
					"missing_project_context",
					"intake_goal dry-run requires a registered Decodex project config or --config.",
				);
			},
		};
		let config = match ServiceConfig::from_path(config_path) {
			Ok(config) => config,
			Err(_) => {
				return mcp::tool_refusal(
					"missing_project_context",
					"intake_goal dry-run could not load the Decodex project config.",
				);
			},
		};
		let workflow = match WorkflowDocument::from_path(config.workflow_path()) {
			Ok(workflow) => workflow,
			Err(_) => {
				return mcp::tool_refusal(
					"missing_project_context",
					"intake_goal dry-run could not load the Decodex workflow contract.",
				);
			},
		};
		let tracker = McpDryRunTracker;

		match program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id,
			team_issue_identifier: params.team_issue_identifier,
			dry_run: true,
			apply: false,
		}) {
			Ok(report) => mcp::tool_success(results::intake_goal_result(&report, mode)),
			Err(_) => mcp::tool_refusal(
				"intake_goal_refused",
				"Goal intake dry-run was refused by Decodex authority checks.",
			),
		}
	}

	fn apply_intake_goal_tool(
		&self,
		contract_id: &str,
		team_issue_identifier: Option<&str>,
	) -> Value {
		let Some(config_path) = self.context.config_path.as_deref() else {
			return mcp::tool_refusal(
				"missing_project_context",
				"intake_goal apply requires a registered Decodex project config or --config.",
			);
		};

		match program_intake::run_goal_intake_command(GoalIntakeCommandRequest {
			config_path: Some(config_path),
			project_id: self.context.project_id.as_deref(),
			contract_id,
			team_issue_identifier,
			dry_run: false,
			apply: true,
		}) {
			Ok(report) => mcp::tool_success(results::intake_goal_result(&report, "apply")),
			Err(_) => mcp::tool_refusal(
				"intake_goal_refused",
				"Goal intake apply was refused by Decodex authority or tracker checks.",
			),
		}
	}
}
