mod constants;
mod context;
mod interrupt;
mod project;
mod render;
mod reports;
mod requests;
mod selection;
mod steer;

pub(crate) use self::{
	constants::DEFAULT_STEER_RESULT_WAIT_TIMEOUT,
	reports::{LaneInspectReport, LaneInterruptReport},
	requests::{LaneInspectRequest, LaneInterruptRequest},
};

use self::reports::LaneRunInspect;
use crate::{
	config::ServiceConfig,
	orchestrator::{self, DEFAULT_STATUS_RUN_LIMIT, LaneSteerReport, LaneSteerRequest},
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
};

pub(crate) fn print_lane_inspect(request: LaneInspectRequest<'_>) -> Result<()> {
	let state_store = runtime::open_runtime_store()?;
	let config = self::project::load_lane_control_project(request.config_path, &state_store)?;
	let report = build_lane_inspect_report(&state_store, &config, request.issue, request.run_id)?;

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", self::render::render_lane_inspect_report(&report));
	}

	Ok(())
}

pub(crate) fn interrupt_lane(request: LaneInterruptRequest<'_>) -> Result<LaneInterruptReport> {
	let state_store = runtime::open_runtime_store()?;
	let config = self::project::load_lane_control_project(request.config_path, &state_store)?;
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
		print!("{}", self::render::render_lane_interrupt_report(&report));
	}

	Ok(report)
}

pub(crate) fn steer_lane(request: LaneSteerRequest<'_>) -> Result<LaneSteerReport> {
	self::steer::validate_lane_steer_request(&request)?;

	let state_store = runtime::open_runtime_store()?;
	let config = self::project::load_lane_control_project_for_optional_id(
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
	let run = self::selection::select_interrupt_lane_run(&snapshot, issue, run_id)?;
	let soft_interrupt = self::interrupt::attempt_soft_lane_interrupt(
		state_store,
		project,
		&run,
		force,
		reason,
		source,
	)?;
	let hard_interrupt =
		if force && self::interrupt::soft_interrupt_allows_hard_fallback(&soft_interrupt, &run) {
			Some(self::interrupt::attempt_hard_lane_interrupt(state_store, &run, reason)?)
		} else {
			None
		};
	let classification = hard_interrupt
		.as_ref()
		.map(|hard| hard.classification.clone())
		.unwrap_or_else(|| soft_interrupt.classification.clone());
	let next_action = self::interrupt::lane_interrupt_next_action(
		&soft_interrupt,
		hard_interrupt.as_ref(),
		force,
	);

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
	self::steer::validate_lane_steer_request(request)?;

	let snapshot = orchestrator::build_operator_status_snapshot(
		project,
		state_store,
		DEFAULT_STATUS_RUN_LIMIT,
	)?;
	let run = self::selection::select_interrupt_lane_run(&snapshot, request.issue, request.run_id)?;

	self::steer::attempt_lane_steer(state_store, project, &run, request)
}
