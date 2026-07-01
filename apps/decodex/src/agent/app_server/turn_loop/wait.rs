#[allow(clippy::wildcard_imports)] use super::*;
use super::{
	completion::{
		adopt_thread_bound_notification_turn_id, handle_turn_execution_notification,
		turn_failure_from_json_rpc_error_response,
	},
	messages::remaining_idle_budget,
};

pub(in crate::agent::app_server) fn flush_pending_messages(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	target_thread_id: Option<&str>,
) -> crate::prelude::Result<()> {
	for message in client.drain_pending() {
		if targets_thread(&message, target_thread_id) {
			recorder.record(message_type(&message), &message.raw)?;

			apply_protocol_message_side_effects(recorder, &message)?;
		}
	}

	Ok(())
}

pub(super) fn wait_for_turn_completion(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<RunOutcome> {
	let control_enabled = request.activity_marker_path.is_some();
	let mut last_activity_at = Instant::now();
	let mut target_turn_id = target_turn_id.to_owned();
	let mut final_output = String::new();
	let mut latest_turn_failure: Option<AppServerTurnFailure> = None;

	loop {
		if control_enabled
			&& let Some(response_turn_id) = handle_pending_turn_control_requests(
				client,
				recorder,
				request,
				target_thread_id,
				&target_turn_id,
			)? {
			recorder.state_store.update_run_turn(recorder.run_id, &response_turn_id)?;
			recorder.set_turn_id(&response_turn_id)?;

			target_turn_id = response_turn_id;
			last_activity_at = Instant::now();
		}

		let idle_timeout = protocol_activity_idle_timeout(
			Some(&recorder.protocol_activity.summary),
			request.timeout,
		);
		let Some(wire_message) = next_turn_wire_message(
			client,
			last_activity_at,
			idle_timeout,
			target_thread_id,
			&target_turn_id,
			latest_turn_failure.as_ref(),
			control_enabled,
		)?
		else {
			continue;
		};

		if !targets_thread(&wire_message, Some(target_thread_id)) {
			tracing::debug!(raw = %wire_message.raw, "Ignoring app-server message for another thread.");

			continue;
		}

		last_activity_at = Instant::now();

		recorder.record(message_type(&wire_message), &wire_message.raw)?;

		apply_protocol_message_side_effects(recorder, &wire_message)?;

		match &wire_message.message {
			JsonRpcMessage::Notification(notification) => {
				adopt_thread_bound_notification_turn_id(
					recorder,
					notification,
					target_thread_id,
					&mut target_turn_id,
				)?;

				if let Some(outcome) = handle_turn_execution_notification(
					notification,
					target_thread_id,
					&target_turn_id,
					&mut final_output,
					&mut latest_turn_failure,
				)? {
					return Ok(outcome);
				}
			},
			JsonRpcMessage::Request(server_request) => handle_turn_execution_request(
				client,
				recorder,
				server_request,
				target_thread_id,
				&target_turn_id,
				request.dynamic_tool_handler,
				request.codex_account_provider,
			)?,
			JsonRpcMessage::Response(_) => ignore_orphan_turn_json_rpc_response(),
			JsonRpcMessage::Error(error) => {
				latest_turn_failure = Some(turn_failure_from_json_rpc_error_response(
					target_thread_id,
					&target_turn_id,
					error,
				));
			},
		}
	}
}

fn next_turn_wire_message(
	client: &mut AppServerClient,
	last_activity_at: Instant,
	timeout: Duration,
	target_thread_id: &str,
	target_turn_id: &str,
	latest_turn_failure: Option<&AppServerTurnFailure>,
	control_enabled: bool,
) -> crate::prelude::Result<Option<WireMessage>> {
	let now = Instant::now();
	let wait_timeout = remaining_idle_budget(last_activity_at, now, timeout).ok_or_else(|| {
		turn_wait_timeout_error(target_thread_id, target_turn_id, latest_turn_failure.cloned())
	})?;
	let recv_timeout =
		if control_enabled { wait_timeout.min(RUN_CONTROL_POLL_INTERVAL) } else { wait_timeout };

	match recv_turn_wire_message(client, recv_timeout, latest_turn_failure) {
		Ok(wire_message) => Ok(Some(wire_message)),
		Err(error)
			if control_enabled
				&& recv_timeout < wait_timeout
				&& is_app_server_output_timeout(&error) =>
			Ok(None),
		Err(error) => Err(error),
	}
}

pub(in crate::agent::app_server) fn is_app_server_output_timeout(error: &Report) -> bool {
	error.downcast_ref::<AppServerOutputTimeout>().is_some()
}

fn handle_turn_execution_request(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: &str,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	codex_account_provider: Option<&dyn CodexAccountProvider>,
) -> crate::prelude::Result<()> {
	handle_server_request_during_turn_execution(
		client,
		recorder,
		request,
		RequestDispatchContext::new(
			RequestWaitPhase::TurnExecution,
			dynamic_tool_handler,
			codex_account_provider,
			Some(target_thread_id),
			Some(target_turn_id),
		),
	)
}

fn ignore_orphan_turn_json_rpc_response() {
	tracing::debug!(
		"Recorded and ignored orphan app-server JSON-RPC response while waiting for turn completion."
	);
}

fn turn_wait_timeout_error(
	target_thread_id: &str,
	target_turn_id: &str,
	latest_turn_failure: Option<AppServerTurnFailure>,
) -> Report {
	let message = format!(
		"Timed out while waiting for turn `{target_turn_id}` on thread `{target_thread_id}`."
	);

	if let Some(failure) = latest_turn_failure {
		return Report::new(failure).wrap_err(message);
	}

	eyre::eyre!(message)
}

fn recv_turn_wire_message(
	client: &mut AppServerClient,
	wait_timeout: Duration,
	latest_turn_failure: Option<&AppServerTurnFailure>,
) -> crate::prelude::Result<WireMessage> {
	match annotate_transport_failure_phase(
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
