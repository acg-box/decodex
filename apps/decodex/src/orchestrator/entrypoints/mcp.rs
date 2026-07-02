use std::{path::Path, time::Duration};

use serde_json::{self, Value};

use crate::{
	config::ServiceConfig,
	orchestrator::{self, AccountActivityMode, LaneSteerRequest, lane_control},
	prelude::{Result, eyre},
	runtime,
};

pub(crate) struct McpLaneSteerRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) expected_turn_id: &'a str,
	pub(crate) message: &'a str,
	pub(crate) source: &'a str,
	pub(crate) wait_timeout: Duration,
}

pub(crate) fn build_mcp_status_resource(config_path: Option<&Path>, limit: usize) -> Result<Value> {
	if limit == 0 {
		eyre::bail!("MCP status resource limit must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = orchestrator::resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Start MCP from a registered checkout or pass --config."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let mut snapshot = orchestrator::build_operator_status_snapshot_with_account_mode(
		&config,
		&state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	snapshot.status_source = Some(String::from("local_runtime"));

	serde_json::to_value(snapshot).map_err(Into::into)
}

pub(crate) fn build_mcp_lane_control_resource(
	config_path: Option<&Path>,
	issue: Option<&str>,
	run_id: Option<&str>,
	limit: usize,
) -> Result<Value> {
	if limit == 0 {
		eyre::bail!("MCP lane-control resource limit must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = orchestrator::resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Start MCP from a registered checkout or pass --config."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;

	if let Some(issue) = issue {
		let report = lane_control::build_lane_inspect_report(&state_store, &config, issue, run_id)?;

		return serde_json::to_value(report).map_err(Into::into);
	}

	let snapshot = orchestrator::build_operator_status_snapshot_with_account_mode(
		&config,
		&state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.lane_control_readback/1",
		"project_id": snapshot.project_id,
		"read_only": true,
		"mutating_tools": [],
		"current_lanes": snapshot.current_lanes,
		"recent_runs": snapshot.recent_runs,
		"post_review_lanes": snapshot.post_review_lanes
	}))
}

pub(crate) fn run_mcp_lane_interrupt(
	config_path: Option<&Path>,
	issue: &str,
	run_id: &str,
	force: bool,
	reason: Option<&str>,
	source: &str,
) -> Result<Value> {
	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = orchestrator::resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Start MCP from a registered checkout or pass --config."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let report = lane_control::interrupt_lane_with_state(
		&state_store,
		&config,
		issue,
		run_id,
		force,
		reason,
		source,
	)?;

	serde_json::to_value(report).map_err(Into::into)
}

pub(crate) fn run_mcp_lane_steer(request: McpLaneSteerRequest<'_>) -> Result<Value> {
	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = orchestrator::resolve_config_path(request.config_path, &state_store)?
	else {
		eyre::bail!(
			"No Decodex project config found. Start MCP from a registered checkout or pass --config."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let lane_request = LaneSteerRequest {
		config_path: Some(&config_path),
		project_id: request.project_id,
		issue: request.issue,
		run_id: request.run_id,
		expected_turn_id: request.expected_turn_id,
		message: request.message,
		source: request.source,
		wait_timeout: request.wait_timeout,
	};
	let report = lane_control::steer_lane_with_state(&state_store, &config, &lane_request)?;

	serde_json::to_value(report).map_err(Into::into)
}
