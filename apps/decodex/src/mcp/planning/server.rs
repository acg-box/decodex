use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	mcp::{
		self, McpServer, TOOL_INTAKE_GOAL, TOOL_RESEARCH_COMPILE, TOOL_RESEARCH_PROMOTE,
		planning::{
			self,
			args::{IntakeGoalToolArgs, ResearchCompileToolArgs, ResearchPromoteToolArgs},
			authority, results,
			tracker::McpDryRunTracker,
		},
	},
	program_intake::{self, GoalIntakeCommandRequest, GoalIntakeRunRequest},
	research_design::{self, ResearchDesignOutcome, ResearchDesignRunInput},
	workflow::WorkflowDocument,
};

impl McpServer {
	pub(in crate::mcp) fn call_research_compile_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ResearchCompileToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_RESEARCH_COMPILE,
					"`mode` must be dry_run or apply, with either `input` or `intent`.",
				);
			},
		};
		let mode =
			match planning::planning_mode(params.mode.as_deref(), "dry_run", TOOL_RESEARCH_COMPILE)
			{
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning::planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_RESEARCH_COMPILE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};

		if mode == "apply" && !planning::planning_authority_present(params.authority.as_ref()) {
			return planning::missing_authority_refusal(
				TOOL_RESEARCH_COMPILE,
				"research_compile apply requires authority.source and authority.reason.",
			);
		}

		let input = match research_compile_input(params) {
			Ok(input) => input,
			Err(result) => return result,
		};
		let report = if mode == "apply" {
			let store = match planning::planning_state_store(&self.context, TOOL_RESEARCH_COMPILE) {
				Ok(store) => store,
				Err(result) => return result,
			};

			research_design::persist_research_design_run(store, &project_id, input)
		} else {
			research_design::dry_run_research_design_compile(input, &project_id)
		};

		match report {
			Ok(report) =>
				mcp::tool_success(results::research_compile_result(&report, mode == "apply", mode)),
			Err(_) => mcp::tool_refusal(
				"research_compile_refused",
				"Research compile input did not satisfy Decodex Decision Contract requirements.",
			),
		}
	}
}

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

impl McpServer {
	pub(in crate::mcp) fn call_research_promote_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ResearchPromoteToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_RESEARCH_PROMOTE,
					"`contractId` is required and `mode` must be dry_run or apply.",
				);
			},
		};
		let Some(contract_id) = mcp::non_empty_string(Some(params.contract_id.as_str())) else {
			return mcp::invalid_tool_arguments(TOOL_RESEARCH_PROMOTE, "`contractId` is required.");
		};

		if !mcp::safe_runtime_identifier(contract_id) {
			return mcp::invalid_tool_arguments(
				TOOL_RESEARCH_PROMOTE,
				"`contractId` must be a safe Decodex runtime identifier.",
			);
		}

		let mode =
			match planning::planning_mode(params.mode.as_deref(), "dry_run", TOOL_RESEARCH_PROMOTE)
			{
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning::planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_RESEARCH_PROMOTE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store = match planning::planning_state_store(&self.context, TOOL_RESEARCH_PROMOTE) {
			Ok(store) => store,
			Err(result) => return result,
		};

		if mode == "dry_run" {
			return match store.decision_contract(&project_id, contract_id) {
				Ok(Some(record)) => mcp::tool_success(results::research_promote_readiness_result(
					record.contract_id(),
					record.status().as_str(),
					record.contract().execution_readiness().ready_for_issue_shaping(),
					false,
					mode,
				)),
				Ok(None) => mcp::tool_refusal(
					"contract_not_found",
					"Decision Contract was not found in the current Decodex project.",
				),
				Err(_) => mcp::tool_refusal(
					"research_promote_refused",
					"Decision Contract readback failed before promotion.",
				),
			};
		}

		let authority = match authority::promotion_authority(params.authority.as_ref()) {
			Ok(authority) => authority,
			Err(result) => return result,
		};
		let accepted_at = match authority.accepted_at {
			Some(accepted_at) => accepted_at.to_owned(),
			None => match OffsetDateTime::now_utc().format(&Rfc3339) {
				Ok(value) => value,
				Err(_) => {
					return mcp::tool_refusal(
						"research_promote_refused",
						"Promotion timestamp could not be prepared.",
					);
				},
			},
		};
		let promotion = match DecisionPromotion::new(
			authority.accepted_by,
			DecisionPromotionActorKind::User,
			accepted_at,
			authority.acceptance_source,
			authority.reason.cloned(),
		) {
			Ok(promotion) => promotion,
			Err(_) => {
				return mcp::tool_refusal(
					"research_promote_refused",
					"Promotion authority did not satisfy Decodex Decision Contract requirements.",
				);
			},
		};

		match research_design::promote_research_design_contract(
			store,
			&project_id,
			contract_id,
			promotion,
		) {
			Ok(record) => mcp::tool_success(results::research_promote_readiness_result(
				record.contract_id(),
				record.status().as_str(),
				record.contract().execution_readiness().ready_for_issue_shaping(),
				true,
				mode,
			)),
			Err(_) => mcp::tool_refusal(
				"research_promote_refused",
				"Decision Contract promotion was refused by Decodex authority checks.",
			),
		}
	}
}

fn research_compile_input(
	params: ResearchCompileToolArgs,
) -> Result<ResearchDesignRunInput, Value> {
	match (params.input, params.intent) {
		(Some(input), None) => Ok(input),
		(None, Some(intent)) => Ok(ResearchDesignRunInput::from_intent(
			intent,
			params.source_issue,
			params.outcome.unwrap_or(ResearchDesignOutcome::NotDecisionReady),
		)),
		(None, None) => Err(mcp::invalid_tool_arguments(
			TOOL_RESEARCH_COMPILE,
			"research_compile requires either `input` or `intent`.",
		)),
		(Some(_), Some(_)) => Err(mcp::invalid_tool_arguments(
			TOOL_RESEARCH_COMPILE,
			"research_compile accepts `input` or `intent`, not both.",
		)),
	}
}
