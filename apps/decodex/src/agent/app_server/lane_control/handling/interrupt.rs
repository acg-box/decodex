use crate::{
	agent::app_server::{
		self, AppServerClient, AppServerRunRequest, LaneControlInterruptResponse, Path,
		PendingLaneControlRequest, RequestDispatchContext, RequestWaitPhase, RunRecorder,
		TurnInterruptRequest,
		lane_control::{errors, recording, rejection},
	},
	prelude::Result,
	run_control,
};

pub(in crate::agent::app_server::lane_control::handling) fn handle_pending_turn_interrupt_request(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	run_request: &AppServerRunRequest<'_>,
	worktree_path: &Path,
	pending: PendingLaneControlRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Result<()> {
	recording::record_lane_interrupt_request(recorder, &pending.request)?;

	if let Some((error_class, message)) = rejection::lane_interrupt_request_rejection(
		run_request,
		&pending.request,
		target_thread_id,
		target_turn_id,
	) {
		let response =
			LaneControlInterruptResponse::rejected(&pending.request, error_class, message);

		recording::record_lane_interrupt_response(recorder, &response)?;
		run_control::write_interrupt_response(worktree_path, &response)?;
		run_control::remove_interrupt_request(&pending.path)?;

		return Ok(());
	}

	let interrupt = TurnInterruptRequest {
		thread_id: pending.request.thread_id.clone(),
		turn_id: pending.request.turn_id.clone(),
	};
	let result = client.interrupt_turn_with_handler(
		interrupt,
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
					Some(target_turn_id),
				),
			)
		},
	);
	let response = match result {
		Ok(value) => LaneControlInterruptResponse::delivered(
			&pending.request,
			run_control::protocol_response_summary(&value),
		),
		Err(error) => LaneControlInterruptResponse::failed(
			&pending.request,
			errors::soft_interrupt_error_class(&error),
			format!("turn/interrupt failed with {}.", errors::soft_interrupt_error_class(&error)),
		),
	};

	recording::record_lane_interrupt_response(recorder, &response)?;
	run_control::write_interrupt_response(worktree_path, &response)?;
	run_control::remove_interrupt_request(&pending.path)?;

	Ok(())
}
