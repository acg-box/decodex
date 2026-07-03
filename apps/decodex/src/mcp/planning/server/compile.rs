use serde_json::Value;

use crate::{
	mcp::{
		self, McpServer, TOOL_RESEARCH_COMPILE,
		planning::{self, args::ResearchCompileToolArgs, results},
	},
	research_design::{self, ResearchDesignOutcome, ResearchDesignRunInput},
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
