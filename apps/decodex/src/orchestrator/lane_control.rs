use std::{
	collections::HashSet,
	io::Error,
	path::{Path, PathBuf},
	process, thread,
	time::{Duration, Instant},
};

use libc::{ESRCH, SIGKILL, SIGTERM, c_int, pid_t};
use serde::Serialize;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, ChildRunRef, DEFAULT_STATUS_RUN_LIMIT, OperatorRunStatus, OperatorStatusSnapshot,
	},
	prelude::{Result, eyre},
	run_control::{
		self, LaneControlInterruptRequest, LaneControlInterruptRequestInput,
		LaneControlInterruptResponse, LaneControlResponseStatus,
	},
	runtime,
	state::{
		RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED, RUN_CONTROL_ACTION_FALLBACK,
		RUN_CONTROL_ACTION_TIMED_OUT, RunControlActionReceipt, RunControlActionRequest, StateStore,
	},
};

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
	active_lease: bool,
	execution_liveness: String,
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
			active_lease: run.active_lease,
			execution_liveness: run.execution_liveness.clone(),
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
			hard_interrupt_available: run.process_id.is_some() && run.process_alive != Some(false),
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

pub(super) fn build_lane_inspect_report(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue: &str,
	run_id: Option<&str>,
) -> Result<LaneInspectReport> {
	let snapshot = orchestrator::build_operator_status_snapshot(
		project,
		state_store,
		DEFAULT_STATUS_RUN_LIMIT,
	)?;
	let runs = matching_lane_runs(&snapshot, issue, run_id);

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
	let hard_interrupt = if force && soft_interrupt_allows_hard_fallback(&soft_interrupt) {
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

fn soft_interrupt_allows_hard_fallback(soft: &LaneSoftInterruptReport) -> bool {
	matches!(soft.status.as_str(), "pending" | "failed" | "unavailable")
		&& soft.error_class.as_deref() != Some("lane_not_active")
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

	for run in snapshot.active_runs.iter().chain(snapshot.recent_runs.iter()) {
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
	orchestrator::operator_run_counts_as_active(run)
		&& run.worktree_path.is_some()
		&& run.thread_id.is_some()
		&& run.turn_id.is_some()
		&& run.control_capability.as_ref().is_some_and(|capability| capability.status == "active")
}

fn attempt_soft_lane_interrupt(
	state_store: &StateStore,
	project: &ServiceConfig,
	run: &OperatorRunStatus,
	force: bool,
	reason: Option<&str>,
	source: &str,
) -> Result<LaneSoftInterruptReport> {
	let Some(worktree_path) = absolute_lane_worktree_path(project, state_store, run)? else {
		return Ok(LaneSoftInterruptReport::unavailable(
			"worktree_missing",
			"Soft interrupt requires the active lane worktree and run-control directory.",
		));
	};
	let Some(thread_id) = run.thread_id.as_deref() else {
		return Ok(LaneSoftInterruptReport::unavailable(
			"thread_id_missing",
			"Soft interrupt requires a recorded app-server thread id.",
		));
	};
	let Some(turn_id) = run.turn_id.as_deref() else {
		return Ok(LaneSoftInterruptReport::unavailable(
			"turn_id_missing",
			"Soft interrupt requires a recorded active app-server turn id.",
		));
	};

	if !orchestrator::operator_run_counts_as_active(run) {
		return Ok(LaneSoftInterruptReport::unavailable(
			"lane_not_active",
			"Soft interrupt only targets active or live local lane runs.",
		));
	}

	let receipt = state_store.resolve_run_control_action(RunControlActionRequest {
		project_id: project.service_id(),
		issue_id: &run.issue_id,
		run_id: &run.run_id,
		attempt_number: run.attempt_number,
		thread_id: Some(thread_id),
		turn_id: Some(turn_id),
		source,
		action: "interrupt",
		timeout_ms: Some(
			i64::try_from(LANE_INTERRUPT_RESPONSE_WAIT.as_millis()).unwrap_or(i64::MAX),
		),
	})?;

	if receipt.outcome() != "accepted" {
		return Ok(LaneSoftInterruptReport::from_control_rejection(&receipt));
	}

	let request = LaneControlInterruptRequest::new(LaneControlInterruptRequestInput {
		project_id: project.service_id(),
		issue_id: &run.issue_id,
		run_id: &run.run_id,
		attempt_number: run.attempt_number,
		thread_id,
		turn_id,
		source,
		reason,
	});

	run_control::write_interrupt_request(&worktree_path, &request)?;

	state_store.append_private_execution_event(
		project.service_id(),
		&run.issue_id,
		&run.run_id,
		run.attempt_number,
		"lane_control/interrupt/requested",
		serde_json::json!({
			"requestId": request.request_id,
			"source": source,
			"method": "turn/interrupt",
			"force": force,
			"reason": reason,
		}),
	)?;

	match run_control::wait_for_interrupt_response(
		&worktree_path,
		&run.run_id,
		&request.request_id,
		LANE_INTERRUPT_RESPONSE_WAIT,
	)? {
		Some(response) => {
			let outcome = match &response.status {
				LaneControlResponseStatus::SoftDelivered => RUN_CONTROL_ACTION_COMPLETED,
				LaneControlResponseStatus::SoftFailed => RUN_CONTROL_ACTION_FAILED,
				LaneControlResponseStatus::Rejected => RUN_CONTROL_ACTION_FAILED,
			};

			state_store.record_run_control_action_outcome(
				&receipt,
				outcome,
				&response.classification,
			)?;

			Ok(LaneSoftInterruptReport::from_response(response))
		},
		None => {
			state_store.record_run_control_action_outcome(
				&receipt,
				RUN_CONTROL_ACTION_TIMED_OUT,
				"soft_interrupt_response_pending",
			)?;

			Ok(LaneSoftInterruptReport {
				attempted: true,
				available: true,
				status: String::from("pending"),
				classification: String::from("soft_interrupt_pending"),
				method: String::from("turn/interrupt"),
				request_id: Some(request.request_id),
				message: String::from(
					"Soft interrupt request was written, but the app-server child has not recorded a response yet.",
				),
				error_class: Some(String::from("soft_interrupt_response_pending")),
				protocol_summary: None,
				response: None,
			})
		},
	}
}

fn attempt_hard_lane_interrupt(
	state_store: &StateStore,
	run: &OperatorRunStatus,
	reason: Option<&str>,
) -> Result<LaneHardInterruptReport> {
	let Some(process_id) = run.process_id else {
		return Ok(LaneHardInterruptReport::unavailable(
			"process_id_missing",
			"Hard interrupt fallback requires a recorded child process id.",
		));
	};

	if process_id == process::id() {
		eyre::bail!("Refusing to hard-interrupt the current Decodex process.");
	}

	let mut signals = Vec::new();
	let mut sent_any_signal = false;

	if send_lane_signal(process_id, SIGTERM)? {
		sent_any_signal = true;

		signals.push(String::from("SIGTERM"));
	}
	if sent_any_signal
		&& !wait_for_lane_process_exit(process_id, LANE_HARD_INTERRUPT_TERM_WAIT)
		&& send_lane_signal(process_id, SIGKILL)?
	{
		signals.push(String::from("SIGKILL"));
	}

	let process_alive_after = Some(orchestrator::process_is_alive(process_id));
	let status = if process_alive_after == Some(false) {
		"sent"
	} else if sent_any_signal {
		"still_alive"
	} else {
		"process_not_found"
	};
	let message = if sent_any_signal {
		String::from("Hard interrupt fallback signaled the recorded child process.")
	} else {
		String::from("Hard interrupt fallback found no signalable child process.")
	};

	state_store.append_private_execution_event(
		&run.project_id,
		&run.issue_id,
		&run.run_id,
		run.attempt_number,
		"lane_control/interrupt",
		serde_json::json!({
			"classification": "hard_interrupt_fallback",
			"status": status,
			"signals": signals,
			"processId": process_id,
			"processAliveAfter": process_alive_after,
			"reason": reason,
		}),
	)?;

	record_hard_interrupt_control_fallback(state_store, run, reason)?;

	if sent_any_signal {
		orchestrator::clear_orphaned_daemon_child_state(
			state_store,
			ChildRunRef {
				issue_id: &run.issue_id,
				run_id: &run.run_id,
				attempt_number: run.attempt_number,
			},
			true,
		)?;
	}

	Ok(LaneHardInterruptReport {
		attempted: true,
		status: String::from(status),
		classification: String::from("hard_interrupt_fallback"),
		signals,
		process_id: Some(process_id),
		process_alive_after,
		message,
		error_class: None,
	})
}

fn record_hard_interrupt_control_fallback(
	state_store: &StateStore,
	run: &OperatorRunStatus,
	_reason: Option<&str>,
) -> Result<()> {
	let receipt = state_store.resolve_run_control_action(RunControlActionRequest {
		project_id: &run.project_id,
		issue_id: &run.issue_id,
		run_id: &run.run_id,
		attempt_number: run.attempt_number,
		thread_id: run.thread_id.as_deref(),
		turn_id: run.turn_id.as_deref(),
		source: "hard_interrupt_fallback",
		action: "interrupt",
		timeout_ms: None,
	})?;

	state_store.record_run_control_action_outcome(
		&receipt,
		RUN_CONTROL_ACTION_FALLBACK,
		"hard_interrupt_fallback",
	)?;

	Ok(())
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

fn send_lane_signal(process_id: u32, signal: c_int) -> Result<bool> {
	let process_id = pid_t::try_from(process_id).map_err(|_error| {
		eyre::eyre!("Recorded child process id is too large for this platform.")
	})?;

	if process_id <= 0 {
		eyre::bail!("Recorded child process id must be positive.");
	}

	match unsafe { libc::kill(process_id, signal) } {
		0 => Ok(true),
		-1 if Error::last_os_error().raw_os_error() == Some(ESRCH) => Ok(false),
		-1 => Err(Error::last_os_error().into()),
		_ => Ok(false),
	}
}

fn wait_for_lane_process_exit(process_id: u32, timeout: Duration) -> bool {
	let started_at = Instant::now();

	while started_at.elapsed() < timeout {
		if !orchestrator::process_is_alive(process_id) {
			return true;
		}

		thread::sleep(Duration::from_millis(100));
	}

	!orchestrator::process_is_alive(process_id)
}

fn lane_interrupt_next_action(
	soft: &LaneSoftInterruptReport,
	hard: Option<&LaneHardInterruptReport>,
	force: bool,
) -> String {
	if let Some(hard) = hard {
		return if hard.status == "unavailable" {
			String::from("Hard fallback was unavailable; inspect the lane before retrying.")
		} else if hard.status == "sent" || hard.status == "process_not_found" {
			String::from(
				"Inspect the lane to confirm the lease and dirty-worktree reconciliation state.",
			)
		} else {
			String::from(
				"The fallback signal did not stop the recorded process; inspect the host process before retrying.",
			)
		};
	}

	match soft.status.as_str() {
		"delivered" =>
			String::from("Inspect the lane until the app-server turn records completion."),
		"pending" =>
			if force {
				String::from("Soft interrupt is pending; forced fallback was not attempted.")
			} else {
				String::from(
					"Re-run inspect shortly, or retry interrupt with --force if operator intent is to kill the process.",
				)
			},
		"rejected" => String::from(
			"Inspect the lane identity before retrying; resolver rejection is not converted into hard fallback.",
		),
		"failed" | "unavailable" => String::from(
			"Retry with --force only if operator intent is to use hard process-kill fallback.",
		),
		_ => String::from("Inspect the lane for the latest run status."),
	}
}

fn render_lane_inspect_report(report: &LaneInspectReport) -> String {
	let mut output = format!(
		"Lane inspect for {} in project {} ({} run{})\n",
		report.issue,
		report.project_id,
		report.matched_run_count,
		if report.matched_run_count == 1 { "" } else { "s" }
	);

	for run in &report.runs {
		output.push_str(&format!(
			"- {} attempt {}: status={}, phase={}, activeLease={}, liveness={}\n",
			run.run_id,
			run.attempt_number,
			run.status,
			run.phase,
			run.active_lease,
			run.execution_liveness
		));
		output.push_str(&format!(
			"  appServer: thread={}, turn={}, softInterruptAvailable={}\n",
			run.thread_id.as_deref().unwrap_or("none"),
			run.turn_id.as_deref().unwrap_or("none"),
			run.soft_interrupt_available
		));
		output.push_str(&format!(
			"  process: pid={}, alive={}, hardInterruptAvailable={} (requires --force)\n",
			run.process_id.map_or_else(|| String::from("none"), |id| id.to_string()),
			run.process_alive.map_or_else(|| String::from("unknown"), |alive| alive.to_string()),
			run.hard_interrupt_available
		));
	}

	output
}

fn render_lane_interrupt_report(report: &LaneInterruptReport) -> String {
	let mut output = format!(
		"Lane interrupt {} for run {}: {}\n",
		report.classification, report.run_id, report.soft_interrupt.message
	);

	if let Some(hard) = &report.hard_interrupt {
		output.push_str(&format!(
			"Hard fallback {}: {} ({})\n",
			hard.status,
			hard.message,
			if hard.signals.is_empty() {
				String::from("no signals")
			} else {
				hard.signals.join(",")
			}
		));
	}

	output.push_str(&format!("Next action: {}\n", report.next_action));

	output
}
