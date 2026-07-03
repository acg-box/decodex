use crate::{
	agent::app_server::{
		self, AppServerClient, AppServerRunRequest, LaneControlInterruptResponse,
		LaneControlSteerResponse, Path, PendingLaneControlRequest, PendingLaneControlSteerRequest,
		RequestDispatchContext, RequestWaitPhase, RunRecorder, TurnInterruptRequest,
		lane_control::{errors, recording, rejection},
	},
	prelude::Result,
	run_control,
};

pub(in crate::agent::app_server) fn handle_pending_turn_control_requests(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Result<Option<String>> {
	let Some(worktree_path) = request.activity_marker_path.as_deref() else {
		return Ok(None);
	};

	for pending in run_control::pending_interrupt_requests(worktree_path, &request.run_id)? {
		handle_pending_turn_interrupt_request(
			client,
			recorder,
			request,
			worktree_path,
			pending,
			target_thread_id,
			target_turn_id,
		)?;
	}
	for pending in run_control::pending_steer_requests(worktree_path, &request.run_id)? {
		if let Some(response_turn_id) = handle_pending_turn_steer_request(
			client,
			recorder,
			request,
			worktree_path,
			pending,
			target_thread_id,
			target_turn_id,
		)? {
			return Ok(Some(response_turn_id));
		}
	}

	Ok(None)
}

fn handle_pending_turn_interrupt_request(
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

fn handle_pending_turn_steer_request(
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
		Ok(value) => {
			LaneControlSteerResponse::delivered(&pending.request, target_turn_id, &value.turn_id)
		},
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
