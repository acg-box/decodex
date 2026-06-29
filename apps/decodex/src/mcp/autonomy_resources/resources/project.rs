use serde_json::{self, Value};

use crate::{
	mcp::{
		DEFAULT_MCP_STATUS_LIMIT, McpError,
		autonomy_resources::{resources::authority, summaries},
	},
	prelude::Result,
	state::StateStore,
};

pub(in crate::mcp) fn mcp_autonomy_project_resource(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Value, McpError> {
	let objectives = state_store
		.recent_autonomy_objectives_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_summary/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": authority::mcp_autonomy_authority_boundary(),
		"objectives": objectives
			.iter()
			.map(|record| summaries::mcp_autonomy_objective_summary(
				record.objective(),
				Some(record.updated_at())
			))
			.collect::<Vec<_>>(),
		"signals": signals
			.iter()
			.map(|record| summaries::mcp_autonomy_signal_summary(
				record.signal(),
				Some(record.updated_at())
			))
			.collect::<Vec<_>>(),
		"proposals": proposals
			.iter()
			.map(|record| summaries::mcp_autonomy_proposal_summary(
				record.proposal(),
				Some(record.updated_at())
			))
			.collect::<Vec<_>>(),
		"evidence": summaries::mcp_autonomy_evidence_summary(&signals, &proposals)
	}))
}
