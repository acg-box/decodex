use std::time::{Duration, Instant};

use color_eyre::eyre::Report;

use crate::{
	agent::{
		app_server::{
			AppServerClient,
			constants::RUN_CONTROL_POLL_INTERVAL,
			runtime_types::RequestWaitPhase,
			transport,
			turn_failure::AppServerTurnFailure,
			turn_loop::{messages, wait::timeout},
		},
		json_rpc::{AppServerOutputTimeout, WireMessage},
	},
	prelude::Result,
};

pub(in crate::agent::app_server::turn_loop::wait) fn next_turn_wire_message(
	client: &mut AppServerClient,
	last_activity_at: Instant,
	timeout: Duration,
	target_thread_id: &str,
	target_turn_id: &str,
	latest_turn_failure: Option<&AppServerTurnFailure>,
	control_enabled: bool,
) -> Result<Option<WireMessage>> {
	let now = Instant::now();
	let wait_timeout =
		messages::remaining_idle_budget(last_activity_at, now, timeout).ok_or_else(|| {
			timeout::turn_wait_timeout_error(
				target_thread_id,
				target_turn_id,
				latest_turn_failure.cloned(),
			)
		})?;
	let recv_timeout =
		if control_enabled { wait_timeout.min(RUN_CONTROL_POLL_INTERVAL) } else { wait_timeout };

	match recv_turn_wire_message(client, recv_timeout, latest_turn_failure) {
		Ok(wire_message) => Ok(Some(wire_message)),
		Err(error)
			if control_enabled
				&& recv_timeout < wait_timeout
				&& timeout::is_app_server_output_timeout(&error) =>
			Ok(None),
		Err(error) => Err(error),
	}
}

fn recv_turn_wire_message(
	client: &mut AppServerClient,
	wait_timeout: Duration,
	latest_turn_failure: Option<&AppServerTurnFailure>,
) -> Result<WireMessage> {
	match transport::annotate_transport_failure_phase(
		client.recv(Some(wait_timeout)),
		RequestWaitPhase::TurnExecution,
	) {
		Ok(wire_message) => Ok(wire_message),
		Err(error) => {
			if error.downcast_ref::<AppServerOutputTimeout>().is_some()
				&& let Some(failure) = latest_turn_failure
			{
				return Err(Report::new(failure.clone())
					.wrap_err("Timed out while waiting for additional app-server output."));
			}

			Err(error)
		},
	}
}
