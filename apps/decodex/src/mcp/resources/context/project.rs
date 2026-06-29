use serde_json::Value;

use crate::{
	mcp::{
		self, DEFAULT_MCP_STATUS_LIMIT, McpContext, McpError, autonomy_resources, observability,
		resources::types::{ResourceContent, ResourceUri},
	},
	orchestrator,
	prelude::Result,
};

impl McpContext {
	pub(super) fn read_project_resource(
		&self,
		uri: &ResourceUri,
	) -> Result<ResourceContent, McpError> {
		let [project_id, resource_kind, rest @ ..] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if Some(project_id.as_str()) != self.project_id.as_deref() {
			return Err(McpError::resource_not_found());
		}
		if resource_kind == "autonomy" {
			let value = self.read_autonomy_project_resource(project_id, rest)?;

			return ResourceContent::mcp_observability_json(&uri.raw, value);
		}

		let Some(config_path) = self.config_path.as_deref() else {
			return Err(McpError::resource_not_found());
		};
		let value = match (resource_kind.as_str(), rest) {
			("status", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal),
			("status_live", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(observability::mcp_status_live_resource)
					.map_err(McpError::internal),
			("activity_tail", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(observability::mcp_activity_tail_resource)
					.map_err(McpError::internal),
			("lane-control", []) => orchestrator::build_mcp_lane_control_resource(
				Some(config_path),
				None,
				None,
				DEFAULT_MCP_STATUS_LIMIT,
			)
			.map(observability::mcp_public_lane_control_readback_resource)
			.map_err(McpError::internal),
			("lane-control", [issue]) if mcp::safe_runtime_identifier(issue) =>
				orchestrator::build_mcp_lane_control_resource(
					Some(config_path),
					Some(issue),
					None,
					DEFAULT_MCP_STATUS_LIMIT,
				)
				.map(observability::mcp_public_lane_inspect_resource)
				.map_err(McpError::internal),
			("lane_inspect", [issue]) if mcp::safe_runtime_identifier(issue) =>
				orchestrator::build_mcp_lane_control_resource(
					Some(config_path),
					Some(issue),
					None,
					DEFAULT_MCP_STATUS_LIMIT,
				)
				.map(observability::mcp_public_lane_inspect_resource)
				.map_err(McpError::internal),
			("runs", [run_id, resource])
				if resource == "events" && mcp::safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| {
						observability::mcp_run_resource(&snapshot, run_id, "events")
					}),
			("runs", [run_id, resource])
				if resource == "protocol_activity" && mcp::safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| {
						observability::mcp_run_resource(&snapshot, run_id, "protocol_activity")
					}),
			("runs", [run_id, resource])
				if resource == "child_agent_activity" && mcp::safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| {
						observability::mcp_run_resource(&snapshot, run_id, "child_agent_activity")
					}),
			("runs", [run_id, resource])
				if resource == "progress_diagnostics" && mcp::safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| {
						observability::mcp_run_resource(&snapshot, run_id, "progress_diagnostics")
					}),
			("pr_review_state", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(observability::mcp_pr_review_state_resource)
					.map_err(McpError::internal),
			_ => return Err(McpError::resource_not_found()),
		}?;

		ResourceContent::mcp_observability_json(&uri.raw, value)
	}

	fn read_autonomy_project_resource(
		&self,
		project_id: &str,
		rest: &[String],
	) -> Result<Value, McpError> {
		let Some(state_store) = self.state_store.as_ref() else {
			return Err(McpError::resource_not_found());
		};

		match rest {
			[] => autonomy_resources::mcp_autonomy_project_resource(state_store, project_id),
			[resource] if resource == "signals" =>
				autonomy_resources::mcp_autonomy_signals_resource(state_store, project_id),
			[resource, signal_id]
				if resource == "signals" && mcp::safe_autonomy_record_identifier(signal_id) =>
				autonomy_resources::mcp_autonomy_signal_resource(state_store, project_id, signal_id),
			[resource] if resource == "proposals" =>
				autonomy_resources::mcp_autonomy_proposals_resource(state_store, project_id),
			[resource, proposal_id]
				if resource == "proposals" && mcp::safe_autonomy_record_identifier(proposal_id) =>
				autonomy_resources::mcp_autonomy_proposal_resource(
					state_store,
					project_id,
					proposal_id,
				),
			[resource] if resource == "evidence" =>
				autonomy_resources::mcp_autonomy_evidence_resource(state_store, project_id),
			[resource, objective_id, selector]
				if resource == "objectives"
					&& mcp::safe_runtime_identifier(objective_id)
					&& selector == "current" =>
				autonomy_resources::mcp_autonomy_current_objective_resource(
					state_store,
					project_id,
					objective_id,
				),
			[resource, objective_id, version]
				if resource == "objectives" && mcp::safe_runtime_identifier(objective_id) =>
				autonomy_resources::mcp_autonomy_objective_version_resource(
					state_store,
					project_id,
					objective_id,
					version,
				),
			_ => Err(McpError::resource_not_found()),
		}
	}
}
