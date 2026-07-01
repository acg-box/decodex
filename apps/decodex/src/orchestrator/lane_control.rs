use std::{
	collections::HashSet,
	path::{Path, PathBuf},
	time::Duration,
};

use serde::Serialize;
use serde_json::Value;

mod interrupt;
mod render;
mod steer;

use self::{
	interrupt::{
		attempt_hard_lane_interrupt, attempt_soft_lane_interrupt, lane_interrupt_next_action,
	},
	render::{render_lane_inspect_report, render_lane_interrupt_report},
	steer::{attempt_lane_steer, validate_lane_steer_request},
};

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, DEFAULT_STATUS_RUN_LIMIT, LaneSteerReport, LaneSteerRequest, OperatorRunStatus,
		OperatorStatusSnapshot,
	},
	prelude::{Result, eyre},
	run_control::{LaneControlInterruptResponse, LaneControlResponseStatus},
	runtime,
	state::{RunControlActionReceipt, StateStore},
};

pub(crate) const DEFAULT_STEER_RESULT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(not(test))]
const LANE_INTERRUPT_RESPONSE_WAIT: Duration = Duration::from_secs(3);
#[cfg(test)]
const LANE_INTERRUPT_RESPONSE_WAIT: Duration = Duration::from_millis(20);
const LANE_HARD_INTERRUPT_TERM_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
pub(crate) struct LaneInspectRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: Option<&'a str>,
	pub(crate) json: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct LaneInterruptRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) force: bool,
	pub(crate) reason: Option<&'a str>,
	pub(crate) json: bool,
	pub(crate) source: &'a str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneInspectReport {
	project_id: String,
	issue: String,
	run_id: Option<String>,
	matched_run_count: usize,
	runs: Vec<LaneRunInspect>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneInterruptReport {
	project_id: String,
	issue: String,
	issue_id: String,
	issue_identifier: Option<String>,
	run_id: String,
	attempt_number: i64,
	force: bool,
	classification: String,
	soft_interrupt: LaneSoftInterruptReport,
	hard_interrupt: Option<LaneHardInterruptReport>,
	next_action: String,
}
impl LaneInterruptReport {
	pub(super) fn http_status_line(&self) -> &'static str {
		if self.soft_interrupt.status == "pending" && self.hard_interrupt.is_none() {
			"202 Accepted"
		} else {
			"200 OK"
		}
	}
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaneRunInspect {
	project_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	run_id: String,
	attempt_number: i64,
	status: String,
	attempt_status: String,
	phase: String,
	wait_reason: Option<String>,
	current_operation: String,
	run_lease: bool,
	execution_liveness: String,
	ownership_state: String,
	liveness_state: String,
	policy_state: String,
	terminalization_state: String,
	lane_control_next_action: String,
	lane_control_conditions: Vec<String>,
	thread_id: Option<String>,
	turn_id: Option<String>,
	thread_status: Option<String>,
	process_id: Option<u32>,
	process_alive: Option<bool>,
	process_liveness_reason: Option<String>,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	event_count: i64,
	worktree_path: Option<String>,
	soft_interrupt_available: bool,
	hard_interrupt_available: bool,
	hard_interrupt_requires_force: bool,
}
impl LaneRunInspect {
	fn from_operator_run(run: &OperatorRunStatus) -> Self {
		Self {
			project_id: run.project_id.clone(),
			issue_id: run.issue_id.clone(),
			issue_identifier: run.issue_identifier.clone(),
			run_id: run.run_id.clone(),
			attempt_number: run.attempt_number,
			status: run.status.clone(),
			attempt_status: run.attempt_status.clone(),
			phase: run.phase.clone(),
			wait_reason: run.wait_reason.clone(),
			current_operation: run.current_operation.clone(),
			run_lease: run.run_lease,
			execution_liveness: run.execution_liveness.clone(),
			ownership_state: run.ownership_state.clone(),
			liveness_state: run.liveness_state.clone(),
			policy_state: run.policy_state.clone(),
			terminalization_state: run.terminalization_state.clone(),
			lane_control_next_action: run.lane_control_next_action.clone(),
			lane_control_conditions: run.lane_control_conditions.clone(),
			thread_id: run.thread_id.clone(),
			turn_id: run.turn_id.clone(),
			thread_status: run.thread_status.clone(),
			process_id: run.process_id,
			process_alive: run.process_alive,
			process_liveness_reason: run.process_liveness_reason.clone(),
			last_event_type: run.last_event_type.clone(),
			last_event_at: run.last_event_at.clone(),
			event_count: run.event_count,
			worktree_path: run.worktree_path.clone(),
			soft_interrupt_available: soft_interrupt_available_for_run(run),
			hard_interrupt_available: hard_interrupt_available_for_run(run),
			hard_interrupt_requires_force: true,
		}
	}
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaneSoftInterruptReport {
	attempted: bool,
	available: bool,
	status: String,
	classification: String,
	method: String,
	request_id: Option<String>,
	message: String,
	error_class: Option<String>,
	protocol_summary: Option<String>,
	response: Option<LaneControlInterruptResponse>,
}
impl LaneSoftInterruptReport {
	fn unavailable(error_class: &str, message: &str) -> Self {
		Self {
			attempted: false,
			available: false,
			status: String::from("unavailable"),
			classification: String::from("soft_interrupt_unavailable"),
			method: String::from("turn/interrupt"),
			request_id: None,
			message: message.to_owned(),
			error_class: Some(error_class.to_owned()),
			protocol_summary: None,
			response: None,
		}
	}

	fn from_response(response: LaneControlInterruptResponse) -> Self {
		let status = match &response.status {
			LaneControlResponseStatus::SoftDelivered => "delivered",
			LaneControlResponseStatus::SoftFailed => "failed",
			LaneControlResponseStatus::Rejected => "rejected",
		};

		Self {
			attempted: true,
			available: true,
			status: String::from(status),
			classification: response.classification.clone(),
			method: response.method.clone(),
			request_id: Some(response.request_id.clone()),
			message: response.message.clone(),
			error_class: response.error_class.clone(),
			protocol_summary: response.protocol_summary.clone(),
			response: Some(response),
		}
	}

	fn from_control_rejection(receipt: &RunControlActionReceipt) -> Self {
		Self {
			attempted: false,
			available: false,
			status: String::from("rejected"),
			classification: String::from("control_request_rejected"),
			method: String::from("turn/interrupt"),
			request_id: None,
			message: format!(
				"Run-control resolver rejected the interrupt request: {}.",
				receipt.reason()
			),
			error_class: Some(receipt.reason().to_owned()),
			protocol_summary: None,
			response: None,
		}
	}
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaneHardInterruptReport {
	attempted: bool,
	status: String,
	classification: String,
	signals: Vec<String>,
	process_id: Option<u32>,
	process_alive_after: Option<bool>,
	message: String,
	error_class: Option<String>,
}
impl LaneHardInterruptReport {
	fn unavailable(error_class: &str, message: &str) -> Self {
		Self {
			attempted: false,
			status: String::from("unavailable"),
			classification: String::from("hard_interrupt_fallback"),
			signals: Vec::new(),
			process_id: None,
			process_alive_after: None,
			message: message.to_owned(),
			error_class: Some(error_class.to_owned()),
		}
	}
}

pub(crate) fn print_lane_inspect(request: LaneInspectRequest<'_>) -> Result<()> {
	let state_store = runtime::open_runtime_store()?;
	let config = load_lane_control_project(request.config_path, &state_store)?;
	let report = build_lane_inspect_report(&state_store, &config, request.issue, request.run_id)?;

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", render_lane_inspect_report(&report));
	}

	Ok(())
}

pub(crate) fn interrupt_lane(request: LaneInterruptRequest<'_>) -> Result<LaneInterruptReport> {
	let state_store = runtime::open_runtime_store()?;
	let config = load_lane_control_project(request.config_path, &state_store)?;
	let report = interrupt_lane_with_state(
		&state_store,
		&config,
		request.issue,
		request.run_id,
		request.force,
		request.reason,
		request.source,
	)?;

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", render_lane_interrupt_report(&report));
	}

	Ok(report)
}

pub(crate) fn steer_lane(request: LaneSteerRequest<'_>) -> Result<LaneSteerReport> {
	validate_lane_steer_request(&request)?;

	let state_store = runtime::open_runtime_store()?;
	let config = load_lane_control_project_for_optional_id(
		request.config_path,
		request.project_id,
		&state_store,
	)?;
	let report = steer_lane_with_state(&state_store, &config, &request)?;

	Ok(report)
}

pub(super) fn build_lane_inspect_report(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue: &str,
	run_id: Option<&str>,
) -> Result<LaneInspectReport> {
	let runs = orchestrator::build_lane_inspect_operator_runs(
		project,
		state_store,
		issue,
		run_id,
		DEFAULT_STATUS_RUN_LIMIT,
	)?;

	if runs.is_empty() {
		eyre::bail!(
			"No local lane matched issue `{}`{} in project `{}`.",
			issue,
			run_id.map(|id| format!(" and run `{id}`")).unwrap_or_default(),
			project.service_id()
		);
	}

	let runs = runs.iter().map(LaneRunInspect::from_operator_run).collect::<Vec<_>>();

	Ok(LaneInspectReport {
		project_id: project.service_id().to_owned(),
		issue: issue.to_owned(),
		run_id: run_id.map(str::to_owned),
		matched_run_count: runs.len(),
		runs,
	})
}

pub(super) fn interrupt_lane_with_state(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue: &str,
	run_id: &str,
	force: bool,
	reason: Option<&str>,
	source: &str,
) -> Result<LaneInterruptReport> {
	let snapshot = orchestrator::build_operator_status_snapshot(
		project,
		state_store,
		DEFAULT_STATUS_RUN_LIMIT,
	)?;
	let run = select_interrupt_lane_run(&snapshot, issue, run_id)?;
	let soft_interrupt =
		attempt_soft_lane_interrupt(state_store, project, &run, force, reason, source)?;
	let hard_interrupt = if force && soft_interrupt_allows_hard_fallback(&soft_interrupt, &run) {
		Some(attempt_hard_lane_interrupt(state_store, &run, reason)?)
	} else {
		None
	};
	let classification = hard_interrupt
		.as_ref()
		.map(|hard| hard.classification.clone())
		.unwrap_or_else(|| soft_interrupt.classification.clone());
	let next_action = lane_interrupt_next_action(&soft_interrupt, hard_interrupt.as_ref(), force);

	Ok(LaneInterruptReport {
		project_id: project.service_id().to_owned(),
		issue: issue.to_owned(),
		issue_id: run.issue_id.clone(),
		issue_identifier: run.issue_identifier.clone(),
		run_id: run.run_id.clone(),
		attempt_number: run.attempt_number,
		force,
		classification,
		soft_interrupt,
		hard_interrupt,
		next_action,
	})
}

pub(super) fn steer_lane_with_state(
	state_store: &StateStore,
	project: &ServiceConfig,
	request: &LaneSteerRequest<'_>,
) -> Result<LaneSteerReport> {
	validate_lane_steer_request(request)?;

	let snapshot = orchestrator::build_operator_status_snapshot(
		project,
		state_store,
		DEFAULT_STATUS_RUN_LIMIT,
	)?;
	let run = select_interrupt_lane_run(&snapshot, request.issue, request.run_id)?;

	attempt_lane_steer(state_store, project, &run, request)
}

fn soft_interrupt_allows_hard_fallback(
	soft: &LaneSoftInterruptReport,
	run: &OperatorRunStatus,
) -> bool {
	if run.phase == "terminal_pending" {
		return false;
	}

	match soft.status.as_str() {
		"pending" | "failed" | "unavailable" =>
			soft.error_class.as_deref() != Some("lane_not_active")
				|| run.process_id.is_some() && run.process_alive != Some(false),
		"rejected" => soft.error_class.as_deref() == Some("run_lease_missing"),
		_ => false,
	}
}

fn load_lane_control_project(
	config_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<ServiceConfig> {
	let Some(config_path) = orchestrator::resolve_config_path(config_path, state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};

	runtime::register_project_config(state_store, &config_path, true)?;

	ServiceConfig::from_path(&config_path)
}

fn load_lane_control_project_for_optional_id(
	config_path: Option<&Path>,
	project_id: Option<&str>,
	state_store: &StateStore,
) -> Result<ServiceConfig> {
	let Some(project_id) = project_id.map(str::trim).filter(|id| !id.is_empty()) else {
		return load_lane_control_project(config_path, state_store);
	};
	let config_path = if let Some(config_path) = config_path {
		ServiceConfig::resolve_project_config_path(config_path)?
	} else {
		state_store
			.list_projects()?
			.into_iter()
			.find(|registration| registration.service_id() == project_id)
			.map(|registration| registration.config_path().to_path_buf())
			.ok_or_else(|| {
				eyre::eyre!(
					"Decodex project `{project_id}` is not registered. Pass --config or run `decodex project add`."
				)
			})?
	};

	runtime::register_project_config(state_store, &config_path, true)?;

	let project = ServiceConfig::from_path(&config_path)?;

	if project.service_id() != project_id {
		eyre::bail!(
			"Lane steer project `{project_id}` did not match config service id `{}`.",
			project.service_id()
		);
	}

	Ok(project)
}

fn select_interrupt_lane_run(
	snapshot: &OperatorStatusSnapshot,
	issue: &str,
	run_id: &str,
) -> Result<OperatorRunStatus> {
	let runs = matching_lane_runs(snapshot, issue, Some(run_id));

	if runs.is_empty() {
		eyre::bail!(
			"No local lane matched issue `{issue}` and run `{run_id}` in project `{}`.",
			snapshot.project_id
		);
	}

	Ok(runs[0].clone())
}

fn matching_lane_runs(
	snapshot: &OperatorStatusSnapshot,
	issue: &str,
	run_id: Option<&str>,
) -> Vec<OperatorRunStatus> {
	let mut seen_run_ids = HashSet::new();
	let mut runs = Vec::new();

	for run in snapshot.current_lanes.iter().chain(snapshot.recent_runs.iter()) {
		if !seen_run_ids.insert(run.run_id.clone()) {
			continue;
		}
		if !lane_issue_matches(run, issue) {
			continue;
		}
		if run_id.is_some_and(|expected| expected != run.run_id) {
			continue;
		}

		runs.push(run.clone());
	}

	runs
}

fn lane_issue_matches(run: &OperatorRunStatus, issue: &str) -> bool {
	let issue = issue.trim();

	run.issue_id == issue
		|| run.issue_identifier.as_deref() == Some(issue)
		|| run
			.issue_identifier
			.as_ref()
			.is_some_and(|identifier| identifier.eq_ignore_ascii_case(issue))
}

fn soft_interrupt_available_for_run(run: &OperatorRunStatus) -> bool {
	orchestrator::operator_run_counts_as_current_lane(run)
		&& run.worktree_path.is_some()
		&& run.thread_id.is_some()
		&& run.turn_id.is_some()
		&& run.control_capability.as_ref().is_some_and(|capability| capability.status == "active")
}

fn hard_interrupt_available_for_run(run: &OperatorRunStatus) -> bool {
	run.phase != "terminal_pending" && run.process_id.is_some() && run.process_alive != Some(false)
}

fn lane_control_operator_context(run: &OperatorRunStatus) -> Value {
	let control_capability = run.control_capability.as_ref().map(|capability| {
		serde_json::json!({
			"project_id": capability.project_id.as_str(),
			"issue_id": capability.issue_id.as_str(),
			"run_id": capability.run_id.as_str(),
			"attempt_number": capability.attempt_number,
			"thread_id": capability.thread_id.as_deref(),
			"turn_id": capability.turn_id.as_deref(),
			"transport": capability.transport.as_str(),
			"channel_path": capability.channel_path.as_str(),
			"status": capability.status.as_str(),
			"published_at": capability.published_at.as_str(),
			"updated_at": capability.updated_at.as_str(),
		})
	});

	serde_json::json!({
		"status": run.status.as_str(),
		"attempt_status": run.attempt_status.as_str(),
		"phase": run.phase.as_str(),
		"wait_reason": run.wait_reason.as_deref(),
		"current_operation": run.current_operation.as_str(),
		"run_lease": run.run_lease,
		"queue_lease_state": run.queue_lease_state.as_str(),
		"execution_liveness": run.execution_liveness.as_str(),
		"ownership_state": run.ownership_state.as_str(),
		"liveness_state": run.liveness_state.as_str(),
		"policy_state": run.policy_state.as_str(),
		"terminalization_state": run.terminalization_state.as_str(),
		"lane_control_next_action": run.lane_control_next_action.as_str(),
		"lane_control_conditions": &run.lane_control_conditions,
		"thread_status": run.thread_status.as_deref(),
		"process_id": run.process_id,
		"process_alive": run.process_alive,
		"process_liveness_reason": run.process_liveness_reason.as_deref(),
		"branch": run.branch_name.as_deref(),
		"worktree_path": run.worktree_path.as_deref(),
		"last_event_type": run.last_event_type.as_deref(),
		"last_event_at": run.last_event_at.as_deref(),
		"event_count": run.event_count,
		"control_capability": control_capability,
	})
}

fn absolute_lane_worktree_path(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
) -> Result<Option<PathBuf>> {
	if let Some(mapping) = state_store.worktree_for_issue(&run.issue_id)? {
		return Ok(Some(mapping.worktree_path().to_path_buf()));
	}

	let Some(worktree_path) = run.worktree_path.as_deref() else {
		return Ok(None);
	};
	let worktree_path = Path::new(worktree_path);

	Ok(Some(if worktree_path.is_absolute() {
		worktree_path.to_path_buf()
	} else {
		project.repo_root().join(worktree_path)
	}))
}
