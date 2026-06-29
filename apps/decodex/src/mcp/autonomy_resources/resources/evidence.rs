use serde_json::{self, Value};

use crate::{
	mcp::{DEFAULT_MCP_STATUS_LIMIT, McpError, autonomy_resources::summaries},
	prelude::Result,
	state::StateStore,
};

pub(in crate::mcp) fn mcp_autonomy_evidence_resource(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Value, McpError> {
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_evidence_resource/1",
		"project_id": project_id,
		"read_only": true,
		"evidence": summaries::mcp_autonomy_evidence_summary(&signals, &proposals)
	}))
}
