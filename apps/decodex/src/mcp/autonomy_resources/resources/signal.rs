use serde_json::{self, Value};

use crate::{
	mcp::{DEFAULT_MCP_STATUS_LIMIT, McpError, autonomy_resources::summaries},
	prelude::Result,
	state::StateStore,
};

pub(in crate::mcp) fn mcp_autonomy_signals_resource(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Value, McpError> {
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signals_resource/1",
		"project_id": project_id,
		"read_only": true,
		"signals": signals
			.iter()
			.map(|record| summaries::mcp_autonomy_signal_summary(
				record.signal(),
				Some(record.updated_at())
			))
			.collect::<Vec<_>>()
	}))
}

pub(in crate::mcp) fn mcp_autonomy_signal_resource(
	state_store: &StateStore,
	project_id: &str,
	signal_id: &str,
) -> Result<Value, McpError> {
	let Some(record) =
		state_store.autonomy_signal(project_id, signal_id).map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signal_resource/1",
		"project_id": project_id,
		"read_only": true,
		"signal": summaries::mcp_autonomy_signal_summary(
			record.signal(),
			Some(record.updated_at())
		)
	}))
}
