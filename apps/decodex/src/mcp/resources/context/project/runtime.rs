use std::path::Path;

use serde_json::Value;

use crate::{
	mcp::{self, DEFAULT_MCP_STATUS_LIMIT, McpError, observability},
	orchestrator,
	prelude::Result,
};

pub(super) fn read_project_runtime_resource(
	config_path: &Path,
	resource_kind: &str,
	rest: &[String],
) -> Result<Value, McpError> {
	match (resource_kind, rest) {
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
			project_lane_inspect_resource(config_path, issue),
		("lane_inspect", [issue]) if mcp::safe_runtime_identifier(issue) =>
			project_lane_inspect_resource(config_path, issue),
		("runs", [run_id, resource])
			if run_resource_allowed(resource) && mcp::safe_runtime_identifier(run_id) =>
			project_run_resource(config_path, run_id, resource),
		("pr_review_state", []) =>
			orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
				.map(observability::mcp_pr_review_state_resource)
				.map_err(McpError::internal),
		_ => Err(McpError::resource_not_found()),
	}
}

fn project_lane_inspect_resource(config_path: &Path, issue: &str) -> Result<Value, McpError> {
	orchestrator::build_mcp_lane_control_resource(
		Some(config_path),
		Some(issue),
		None,
		DEFAULT_MCP_STATUS_LIMIT,
	)
	.map(observability::mcp_public_lane_inspect_resource)
	.map_err(McpError::internal)
}

fn project_run_resource(
	config_path: &Path,
	run_id: &str,
	resource: &str,
) -> Result<Value, McpError> {
	orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)
		.and_then(|snapshot| observability::mcp_run_resource(&snapshot, run_id, resource))
}

fn run_resource_allowed(resource: &str) -> bool {
	matches!(
		resource,
		"events" | "protocol_activity" | "child_agent_activity" | "progress_diagnostics"
	)
}
