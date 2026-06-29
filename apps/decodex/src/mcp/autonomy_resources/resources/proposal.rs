use serde_json::{self, Value};

use crate::{
	mcp::{DEFAULT_MCP_STATUS_LIMIT, McpError, autonomy_resources::summaries},
	prelude::Result,
	state::StateStore,
};

pub(in crate::mcp) fn mcp_autonomy_proposals_resource(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Value, McpError> {
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposals_resource/1",
		"project_id": project_id,
		"read_only": true,
		"proposals": proposals
			.iter()
			.map(|record| summaries::mcp_autonomy_proposal_summary(
				record.proposal(),
				Some(record.updated_at())
			))
			.collect::<Vec<_>>()
	}))
}

pub(in crate::mcp) fn mcp_autonomy_proposal_resource(
	state_store: &StateStore,
	project_id: &str,
	proposal_id: &str,
) -> Result<Value, McpError> {
	let Some(record) =
		state_store.autonomy_proposal(project_id, proposal_id).map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposal_resource/1",
		"project_id": project_id,
		"read_only": true,
		"proposal": summaries::mcp_autonomy_proposal_summary(
			record.proposal(),
			Some(record.updated_at())
		)
	}))
}
