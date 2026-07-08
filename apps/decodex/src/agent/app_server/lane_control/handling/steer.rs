use crate::{
	agent::app_server::{
		self, AppServerClient, AppServerRunRequest, LaneControlSteerResponse, Path,
		PendingLaneControlSteerRequest, RequestDispatchContext, RequestWaitPhase, RunRecorder,
		lane_control::{errors, recording, rejection},
	},
	prelude::Result,
	run_control,
};

pub(in crate::agent::app_server::lane_control::handling) fn handle_pending_turn_steer_request(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	run_request: &AppServerRunRequest<'_>,
	worktree_path: &Path,
	pending: PendingLaneControlSteerRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Result<Option<String>> {
	recording::record_lane_steer_request(recorder, &pending.request)?;

	if let Some((error_class, message)) = rejection::lane_steer_request_rejection(
		run_request,
		&pending.request,
		target_thread_id,
		target_turn_id,
	) {
		let response = LaneControlSteerResponse::rejected(
			&pending.request,
			target_turn_id,
			error_class,
			message,
		);

		recording::record_lane_steer_response(
			recorder,
			&response,
			Some(pending.request.audit_record_id),
		)?;
		run_control::write_steer_response(worktree_path, &response)?;
		run_control::remove_steer_request(&pending.path)?;

		return Ok(None);
	}

	let result = client.steer_turn_with_handler(
		app_server::build_turn_steer_request(
			&pending.request.thread_id,
			&pending.request.expected_turn_id,
			&pending.request.message,
		),
		|connection, wire_message, server_request| {
			app_server::handle_server_request_while_waiting(
				connection,
				recorder,
				wire_message,
				server_request,
				RequestDispatchContext::new(
					RequestWaitPhase::TurnExecution,
					run_request.dynamic_tool_handler,
					run_request.codex_account_provider,
					Some(target_thread_id),
					None,
				),
			)
		},
	);
	let response = match result {
		Ok(value) =>
			LaneControlSteerResponse::delivered(&pending.request, target_turn_id, &value.turn_id),
		Err(error) => {
			let error_class = errors::steer_error_class(&error);

			LaneControlSteerResponse::failed(
				&pending.request,
				target_turn_id,
				error_class,
				format!("turn/steer failed with {error_class}."),
			)
		},
	};
	let response_turn_id = response.response_turn_id.clone();

	recording::record_lane_steer_response(
		recorder,
		&response,
		Some(pending.request.audit_record_id),
	)?;
	run_control::write_steer_response(worktree_path, &response)?;
	run_control::remove_steer_request(&pending.path)?;

	Ok(response_turn_id)
}
