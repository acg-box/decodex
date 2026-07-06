use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	mcp::{
		self, McpServer, TOOL_RESEARCH_PROMOTE,
		planning::{self, args::ResearchPromoteToolArgs, authority, results},
	},
	research_design,
};

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
