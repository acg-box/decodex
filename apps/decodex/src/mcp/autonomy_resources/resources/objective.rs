use serde_json::{self, Value};

use crate::{
	mcp::{
		McpError,
		autonomy_resources::{resources::authority, summaries},
	},
	prelude::Result,
	state::StateStore,
};

pub(in crate::mcp) fn mcp_autonomy_current_objective_resource(
	state_store: &StateStore,
	project_id: &str,
	objective_id: &str,
) -> Result<Value, McpError> {
	let Some(record) = state_store
		.current_accepted_autonomy_objective(project_id, objective_id)
		.map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_resource/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": authority::mcp_autonomy_authority_boundary(),
		"objective": summaries::mcp_autonomy_objective_summary(
			record.objective(),
			Some(record.updated_at())
		)
	}))
}

pub(in crate::mcp) fn mcp_autonomy_objective_version_resource(
	state_store: &StateStore,
	project_id: &str,
	objective_id: &str,
	version: &str,
) -> Result<Value, McpError> {
	let version = version.parse::<u64>().map_err(|_| McpError::resource_not_found())?;
	let Some(record) = state_store
		.autonomy_objective(project_id, objective_id, version)
		.map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_resource/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": authority::mcp_autonomy_authority_boundary(),
		"objective": summaries::mcp_autonomy_objective_summary(
			record.objective(),
			Some(record.updated_at())
		)
	}))
}
