use std::{
	io::Error,
	process, thread,
	time::{Duration, Instant},
};

use libc::{ESRCH, SIGKILL, SIGTERM, c_int, pid_t};

use crate::orchestrator::lane_control::{
	constants::{LANE_HARD_INTERRUPT_TERM_WAIT, LANE_INTERRUPT_RESPONSE_WAIT},
	context::{self},
	reports::{LaneHardInterruptReport, LaneSoftInterruptReport},
};
use crate::{
	config::ServiceConfig,
	orchestrator::{self, ChildRunRef, OperatorRunStatus},
	prelude::{Result, eyre},
	run_control::{
		self, LaneControlInterruptRequest, LaneControlInterruptRequestInput,
		LaneControlResponseStatus,
	},
	state::{
		RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED, RUN_CONTROL_ACTION_FALLBACK,
		RUN_CONTROL_ACTION_TIMED_OUT, RunControlActionReceipt, RunControlActionRequest, StateStore,
	},
};

pub(super) fn attempt_soft_lane_interrupt(
	state_store: &StateStore,
	project: &ServiceConfig,
	run: &OperatorRunStatus,
	force: bool,
	reason: Option<&str>,
	source: &str,
) -> Result<LaneSoftInterruptReport> {
	let Some(worktree_path) = context::absolute_lane_worktree_path(project, state_store, run)?
	else {
		return Ok(LaneSoftInterruptReport::unavailable(
			"worktree_missing",
			"Soft interrupt requires the current lane worktree and run-control directory.",
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

	if !orchestrator::operator_run_counts_as_current_lane(run) {
		return Ok(LaneSoftInterruptReport::unavailable(
			"lane_not_active",
			"Soft interrupt only targets current or live local lane runs.",
		));
	}

	let receipt = resolve_soft_interrupt_control_action(
		state_store,
		project,
		run,
		thread_id,
		turn_id,
		source,
	)?;

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

pub(super) fn attempt_hard_lane_interrupt(
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

pub(super) fn lane_interrupt_next_action(
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
		"delivered" => {
			String::from("Inspect the lane until the app-server turn records completion.")
		},
		"pending" => {
			if force {
				String::from("Soft interrupt is pending; forced fallback was not attempted.")
			} else {
				String::from(
					"Re-run inspect shortly, or retry interrupt with --force if operator intent is to kill the process.",
				)
			}
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

pub(super) fn soft_interrupt_allows_hard_fallback(
	soft: &LaneSoftInterruptReport,
	run: &OperatorRunStatus,
) -> bool {
	if run.phase == "terminal_pending" {
		return false;
	}

	match soft.status.as_str() {
		"pending" | "failed" | "unavailable" => {
			soft.error_class.as_deref() != Some("lane_not_active")
				|| run.process_id.is_some() && run.process_alive != Some(false)
		},
		"rejected" => soft.error_class.as_deref() == Some("run_lease_missing"),
		_ => false,
	}
}

fn resolve_soft_interrupt_control_action(
	state_store: &StateStore,
	project: &ServiceConfig,
	run: &OperatorRunStatus,
	thread_id: &str,
	turn_id: &str,
	source: &str,
) -> Result<RunControlActionReceipt> {
	let context = context::lane_control_operator_context(run);

	state_store.resolve_run_control_action(RunControlActionRequest {
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
		metadata: None,
		context: Some(&context),
	})
}

fn record_hard_interrupt_control_fallback(
	state_store: &StateStore,
	run: &OperatorRunStatus,
	_reason: Option<&str>,
) -> Result<()> {
	let context = context::lane_control_operator_context(run);
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
		metadata: None,
		context: Some(&context),
	})?;

	state_store.record_run_control_action_outcome(
		&receipt,
		RUN_CONTROL_ACTION_FALLBACK,
		"hard_interrupt_fallback",
	)?;

	Ok(())
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
