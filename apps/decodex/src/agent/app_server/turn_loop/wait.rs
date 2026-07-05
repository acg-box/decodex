mod pending;
mod request;
mod timeout;
mod wire;

pub(in crate::agent::app_server) use self::{
	pending::flush_pending_messages, timeout::is_app_server_output_timeout,
};

use std::time::Instant;

use crate::{
	agent::{
		app_server::{
			AppServerClient, activity, lane_control,
			runtime_types::{AppServerRunRequest, RunRecorder},
			server_requests,
			turn_failure::AppServerTurnFailure,
			turn_loop::{RunOutcome, completion, messages},
		},
		json_rpc::JsonRpcMessage,
	},
	prelude::Result,
};

pub(super) fn wait_for_turn_completion(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Result<RunOutcome> {
	let control_enabled = request.activity_marker_path.is_some();
	let mut last_activity_at = Instant::now();
	let mut target_turn_id = target_turn_id.to_owned();
	let mut final_output = String::new();
	let mut latest_turn_failure: Option<AppServerTurnFailure> = None;

	loop {
		if control_enabled
			&& let Some(response_turn_id) = lane_control::handle_pending_turn_control_requests(
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

		let idle_timeout = activity::protocol_activity_idle_timeout(
			Some(&recorder.protocol_activity.summary),
			request.timeout,
		);
		let Some(wire_message) = wire::next_turn_wire_message(
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

		if !messages::targets_thread(&wire_message, Some(target_thread_id)) {
			tracing::debug!(raw = %wire_message.raw, "Ignoring app-server message for another thread.");

			continue;
		}

		last_activity_at = Instant::now();

		recorder.record(messages::message_type(&wire_message), &wire_message.raw)?;

		server_requests::apply_protocol_message_side_effects(recorder, &wire_message)?;

		match &wire_message.message {
			JsonRpcMessage::Notification(notification) => {
				completion::adopt_thread_bound_notification_turn_id(
					recorder,
					notification,
					target_thread_id,
					&mut target_turn_id,
				)?;

				if let Some(outcome) = completion::handle_turn_execution_notification(
					notification,
					target_thread_id,
					&target_turn_id,
					&mut final_output,
					&mut latest_turn_failure,
				)? {
					return Ok(outcome);
				}
			},
			JsonRpcMessage::Request(server_request) => request::handle_turn_execution_request(
				client,
				recorder,
				server_request,
				target_thread_id,
				&target_turn_id,
				request.dynamic_tool_handler,
				request.codex_account_provider,
			)?,
			JsonRpcMessage::Response(_) => request::ignore_orphan_turn_json_rpc_response(),
			JsonRpcMessage::Error(error) => {
				latest_turn_failure = Some(completion::turn_failure_from_json_rpc_error_response(
					target_thread_id,
					&target_turn_id,
					error,
				));
			},
		}
	}
}
