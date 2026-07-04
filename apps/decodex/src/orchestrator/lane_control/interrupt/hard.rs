use std::{
	io::Error,
	process, thread,
	time::{Duration, Instant},
};

use libc::{ESRCH, SIGKILL, SIGTERM, c_int, pid_t};

use crate::{
	orchestrator::{
		self, ChildRunRef, OperatorRunStatus,
		lane_control::{
			constants::LANE_HARD_INTERRUPT_TERM_WAIT, context, reports::LaneHardInterruptReport,
		},
	},
	prelude::{Result, eyre},
	state::{RUN_CONTROL_ACTION_FALLBACK, RunControlActionRequest, StateStore},
};

pub(in crate::orchestrator::lane_control) fn attempt_hard_lane_interrupt(
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

	record_hard_interrupt_control_fallback(state_store, run)?;

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
