use serde::Deserialize;
use serde_json::{self, Value};

use crate::{
	mcp::{self, DEFAULT_MCP_STATUS_LIMIT, TOOL_OBSERVE, observability, server::core::McpServer},
	orchestrator,
};

impl McpServer {
	pub(super) fn call_observe_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ObserveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_OBSERVE,
					"`issue`, `runId`, and `limit` are the only supported observe arguments.",
				);
			},
		};
		let limit = params.limit.unwrap_or(DEFAULT_MCP_STATUS_LIMIT);

		if limit == 0 {
			return mcp::tool_refusal("invalid_limit", "`limit` must be greater than zero.");
		}

		let observability_result = if params.issue.as_deref().is_some() {
			orchestrator::build_mcp_lane_control_resource(
				self.context.config_path.as_deref(),
				params.issue.as_deref(),
				params.run_id.as_deref(),
				limit,
			)
			.map(observability::mcp_public_lane_inspect_resource)
		} else {
			orchestrator::build_mcp_status_resource(self.context.config_path.as_deref(), limit)
				.map(observability::mcp_status_live_resource)
		};
		let mut value = match observability_result {
			Ok(value) => value,
			Err(_) => {
				return mcp::tool_refusal(
					"observability_unavailable",
					"Decodex observability requires a registered project config or --config.",
				);
			},
		};

		observability::sanitize_mcp_observability_value(&mut value);

		mcp::tool_success(serde_json::json!({
			"schema": "decodex.mcp.observe_result/1",
			"status": "ok",
			"capability_profile": "observe",
			"observability": value
		}))
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObserveToolArgs {
	issue: Option<String>,
	run_id: Option<String>,
	limit: Option<usize>,
}
