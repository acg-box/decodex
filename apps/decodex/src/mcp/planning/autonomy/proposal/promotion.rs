use serde_json::{self, Value};

use crate::mcp::{
	self, McpServer, TOOL_AUTONOMY_REQUEST_PROMOTION,
	planning::{
		self,
		autonomy::{args::AutonomyRequestPromotionToolArgs, results},
	},
};

impl McpServer {
	pub(in crate::mcp) fn call_autonomy_request_promotion_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyRequestPromotionToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_AUTONOMY_REQUEST_PROMOTION,
					"`proposalId` and optional `mode` are required.",
				);
			},
		};
		let Some(proposal_id) = mcp::non_empty_string(Some(params.proposal_id.as_str())) else {
			return mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"`proposalId` is required.",
			);
		};

		if !mcp::safe_autonomy_record_identifier(proposal_id) {
			return mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"`proposalId` must be a safe Decodex autonomy identifier.",
			);
		}

		let mode = match planning::planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_REQUEST_PROMOTION,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning::planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_REQUEST_PROMOTION,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store =
			match planning::planning_state_store(&self.context, TOOL_AUTONOMY_REQUEST_PROMOTION) {
				Ok(store) => store,
				Err(result) => return result,
			};
		let record = match store.autonomy_proposal(&project_id, proposal_id) {
			Ok(Some(record)) => record,
			Ok(None) => {
				return mcp::tool_refusal(
					"proposal_not_found",
					"Autonomy proposal was not found in the current Decodex project.",
				);
			},
			Err(error) => {
				return mcp::tool_refusal(
					"autonomy_promotion_refused",
					format!("Autonomy proposal readback failed closed: {error}"),
				);
			},
		};

		if mode == "dry_run" {
			return mcp::tool_success(results::autonomy_promotion_request_result(
				&project_id,
				record.proposal(),
				mode,
				false,
				None,
			));
		}

		let Some(authority) = params.authority else {
			return planning::missing_authority_refusal(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"autonomy_request_promotion apply requires explicit proposal acceptance authority.",
			);
		};
		let authority = match authority.into_decision_bridge_authority() {
			Ok(authority) => authority,
			Err(result) => return result,
		};

		match store.accept_autonomy_proposal_as_decision_contract_candidate(
			&project_id,
			proposal_id,
			authority,
		) {
			Ok(contract) => mcp::tool_success(results::autonomy_promotion_request_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(contract.contract_id()),
			)),
			Err(error) => mcp::tool_refusal(
				"autonomy_promotion_refused",
				format!(
					"Autonomy proposal promotion request was refused by Decodex authority checks: {error}"
				),
			),
		}
	}
}
