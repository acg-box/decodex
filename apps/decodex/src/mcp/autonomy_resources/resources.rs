use serde_json::{self, Value};

use crate::{
	mcp::{DEFAULT_MCP_STATUS_LIMIT, McpError, autonomy_resources::summaries},
	prelude::Result,
	state::StateStore,
};

pub(super) fn mcp_autonomy_authority_boundary() -> Value {
	serde_json::json!({
		"mcp_authentication": "access_boundary_only",
		"capability_profile": "tool_visibility_boundary_only",
		"acceptance_authority": "explicit_human_or_trusted_accepted_project_policy_required",
		"execution_authority": "Decision Contract promotion and Program Intake remain separate"
	})
}

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
		"authority_boundary": mcp_autonomy_authority_boundary(),
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
		"authority_boundary": mcp_autonomy_authority_boundary(),
		"objective": summaries::mcp_autonomy_objective_summary(
			record.objective(),
			Some(record.updated_at())
		)
	}))
}

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
		"authority_boundary": mcp_autonomy_authority_boundary(),
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
