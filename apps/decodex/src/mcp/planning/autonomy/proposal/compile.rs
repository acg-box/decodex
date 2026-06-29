use serde_json::{self, Value};

use crate::mcp::{
	self, McpServer, TOOL_AUTONOMY_COMPILE_PROPOSAL,
	planning::{
		self,
		autonomy::{args::AutonomyCompileProposalToolArgs, results},
	},
};

impl McpServer {
	pub(in crate::mcp) fn call_autonomy_compile_proposal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyCompileProposalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_AUTONOMY_COMPILE_PROPOSAL,
					"`proposal`, `signalIds`, and optional `mode` are required.",
				);
			},
		};
		let mode = match planning::planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning::planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let input = match params.proposal.into_compile_input(&project_id) {
			Ok(input) => input,
			Err(result) => return result,
		};

		if mode == "apply" && !planning::planning_authority_present(params.authority.as_ref()) {
			return planning::missing_authority_refusal(
				TOOL_AUTONOMY_COMPILE_PROPOSAL,
				"autonomy_compile_proposal apply requires authority.source and authority.reason.",
			);
		}

		let store =
			match planning::planning_state_store(&self.context, TOOL_AUTONOMY_COMPILE_PROPOSAL) {
				Ok(store) => store,
				Err(result) => return result,
			};
		let proposal = match store.compile_autonomy_proposal_dry_run(input, &params.signal_ids) {
			Ok(proposal) => proposal,
			Err(error) => {
				return mcp::tool_refusal(
					"autonomy_proposal_refused",
					format!("Autonomy proposal compile was refused: {error}"),
				);
			},
		};

		if mode == "dry_run" {
			return mcp::tool_success(results::autonomy_proposal_tool_result(
				&project_id,
				&proposal,
				mode,
				false,
				None,
			));
		}

		match store.record_autonomy_proposal(&project_id, proposal) {
			Ok(record) => mcp::tool_success(results::autonomy_proposal_tool_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => mcp::tool_refusal(
				"autonomy_proposal_refused",
				format!(
					"Autonomy proposal persistence was refused by Decodex authority checks: {error}"
				),
			),
		}
	}
}
