//! App-server lane-control request handling during active turns.

use super::*;

pub(super) fn handle_pending_turn_control_requests(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<Option<String>> {
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
) -> crate::prelude::Result<()> {
	record_lane_interrupt_request(recorder, &pending.request)?;

	if let Some((error_class, message)) = lane_interrupt_request_rejection(
		run_request,
		&pending.request,
		target_thread_id,
		target_turn_id,
	) {
		let response =
			LaneControlInterruptResponse::rejected(&pending.request, error_class, message);

		record_lane_interrupt_response(recorder, &response)?;

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
			handle_server_request_while_waiting(
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
			soft_interrupt_error_class(&error),
			format!("turn/interrupt failed with {}.", soft_interrupt_error_class(&error)),
		),
	};

	record_lane_interrupt_response(recorder, &response)?;

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
) -> crate::prelude::Result<Option<String>> {
	record_lane_steer_request(recorder, &pending.request)?;

	if let Some((error_class, message)) = lane_steer_request_rejection(
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

		record_lane_steer_response(recorder, &response, Some(pending.request.audit_record_id))?;

		run_control::write_steer_response(worktree_path, &response)?;
		run_control::remove_steer_request(&pending.path)?;

		return Ok(None);
	}

	let result = client.steer_turn_with_handler(
		build_turn_steer_request(
			&pending.request.thread_id,
			&pending.request.expected_turn_id,
			&pending.request.message,
		),
		|connection, wire_message, server_request| {
			handle_server_request_while_waiting(
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
			let error_class = steer_error_class(&error);

			LaneControlSteerResponse::failed(
				&pending.request,
				target_turn_id,
				error_class,
				format!("turn/steer failed with {error_class}."),
			)
		},
	};
	let response_turn_id = response.response_turn_id.clone();

	record_lane_steer_response(recorder, &response, Some(pending.request.audit_record_id))?;

	run_control::write_steer_response(worktree_path, &response)?;
	run_control::remove_steer_request(&pending.path)?;

	Ok(response_turn_id)
}

fn record_lane_interrupt_request(
	recorder: &mut RunRecorder<'_>,
	request: &LaneControlInterruptRequest,
) -> crate::prelude::Result<()> {
	recorder.record(
		"lane_control/interrupt/request",
		&serde_json::json!({
			"requestId": request.request_id,
			"projectId": request.project_id,
			"issueId": request.issue_id,
			"runId": request.run_id,
			"attemptNumber": request.attempt_number,
			"threadId": request.thread_id,
			"turnId": request.turn_id,
			"source": request.source,
			"reason": request.reason,
		})
		.to_string(),
	)
}

fn record_lane_interrupt_response(
	recorder: &mut RunRecorder<'_>,
	response: &LaneControlInterruptResponse,
) -> crate::prelude::Result<()> {
	recorder.record(
		"lane_control/interrupt/response",
		&serde_json::json!({
			"requestId": response.request_id,
			"projectId": response.project_id,
			"issueId": response.issue_id,
			"runId": response.run_id,
			"attemptNumber": response.attempt_number,
			"threadId": response.thread_id,
			"turnId": response.turn_id,
			"status": response.status,
			"classification": response.classification,
			"method": response.method,
			"errorClass": response.error_class,
			"protocolSummary": response.protocol_summary,
		})
		.to_string(),
	)?;
	recorder.state_store.append_private_execution_event(
		&response.project_id,
		&response.issue_id,
		&response.run_id,
		response.attempt_number,
		"lane_control/interrupt",
		serde_json::json!({
			"requestId": response.request_id,
			"status": response.status,
			"classification": response.classification,
			"method": response.method,
			"errorClass": response.error_class,
			"protocolSummary": response.protocol_summary,
			"message": response.message,
		}),
	)?;

	Ok(())
}

fn record_lane_steer_request(
	recorder: &mut RunRecorder<'_>,
	request: &LaneControlSteerRequest,
) -> crate::prelude::Result<()> {
	recorder.record(
		"lane_control/steer/request",
		&serde_json::json!({
			"requestId": request.request_id,
			"auditRecordId": request.audit_record_id,
			"projectId": request.project_id,
			"issueId": request.issue_id,
			"runId": request.run_id,
			"attemptNumber": request.attempt_number,
			"threadId": request.thread_id,
			"expectedTurnId": request.expected_turn_id,
			"source": request.source,
			"messageByteCount": request.message_byte_count,
			"messageLineCount": request.message_line_count,
		})
		.to_string(),
	)
}

fn record_lane_steer_response(
	recorder: &mut RunRecorder<'_>,
	response: &LaneControlSteerResponse,
	parent_record_id: Option<i64>,
) -> crate::prelude::Result<()> {
	let outcome = match &response.status {
		LaneControlSteerResponseStatus::Delivered => RUN_CONTROL_ACTION_COMPLETED,
		LaneControlSteerResponseStatus::Failed | LaneControlSteerResponseStatus::Rejected => {
			RUN_CONTROL_ACTION_FAILED
		},
	};
	let metadata = serde_json::json!({
		"requestId": response.request_id,
		"outcome": outcome,
		"reason": response.classification,
		"failureClass": response.error_class,
		"expectedTurnId": response.expected_turn_id,
		"currentTurnId": response.current_turn_id,
		"responseTurnId": response.response_turn_id,
	});

	recorder.record("turn/steer", &metadata.to_string())?;
	recorder.record(
		"lane_control/steer/response",
		&serde_json::json!({
			"requestId": response.request_id,
			"projectId": response.project_id,
			"issueId": response.issue_id,
			"runId": response.run_id,
			"attemptNumber": response.attempt_number,
			"threadId": response.thread_id,
			"expectedTurnId": response.expected_turn_id,
			"currentTurnId": response.current_turn_id,
			"responseTurnId": response.response_turn_id,
			"status": &response.status,
			"classification": response.classification,
			"method": response.method,
			"errorClass": response.error_class,
		})
		.to_string(),
	)?;
	recorder.state_store.record_run_control_action_delivery_outcome(
		RunControlActionOutcomeRequest {
			project_id: &response.project_id,
			issue_id: &response.issue_id,
			run_id: &response.run_id,
			attempt_number: response.attempt_number,
			thread_id: Some(&response.thread_id),
			turn_id: Some(&response.expected_turn_id),
			current_thread_id: Some(&response.thread_id),
			current_turn_id: response.current_turn_id.as_deref(),
			source: "app_server_child",
			action: "steer",
			outcome,
			reason: &response.classification,
			parent_record_id,
			timeout_ms: None,
			metadata: Some(&metadata),
			channel: None,
		},
	)?;
	recorder.state_store.append_private_execution_event(
		&response.project_id,
		&response.issue_id,
		&response.run_id,
		response.attempt_number,
		"lane_control/steer",
		serde_json::json!({
			"requestId": response.request_id,
			"status": &response.status,
			"classification": response.classification,
			"method": response.method,
			"errorClass": response.error_class,
			"expectedTurnId": response.expected_turn_id,
			"currentTurnId": response.current_turn_id,
			"responseTurnId": response.response_turn_id,
			"message": response.message,
		}),
	)?;

	Ok(())
}

fn lane_interrupt_request_rejection(
	run_request: &AppServerRunRequest<'_>,
	request: &LaneControlInterruptRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Option<(&'static str, String)> {
	if request.project_id != run_request.project_id {
		return Some((
			"project_mismatch",
			format!(
				"Control request targeted project `{}`, but this run belongs to `{}`.",
				request.project_id, run_request.project_id
			),
		));
	}
	if request.issue_id != run_request.issue_id {
		return Some((
			"issue_mismatch",
			format!(
				"Control request targeted issue `{}`, but this run belongs to `{}`.",
				request.issue_id, run_request.issue_id
			),
		));
	}
	if request.run_id != run_request.run_id {
		return Some((
			"run_mismatch",
			format!(
				"Control request targeted run `{}`, but this run is `{}`.",
				request.run_id, run_request.run_id
			),
		));
	}
	if request.attempt_number != run_request.attempt_number {
		return Some((
			"attempt_mismatch",
			format!(
				"Control request targeted attempt `{}`, but this run is attempt `{}`.",
				request.attempt_number, run_request.attempt_number
			),
		));
	}
	if request.thread_id != target_thread_id {
		return Some((
			"thread_mismatch",
			format!(
				"Control request targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				request.thread_id
			),
		));
	}
	if request.turn_id != target_turn_id {
		return Some((
			"turn_mismatch",
			format!(
				"Control request targeted turn `{}`, but the active turn is `{target_turn_id}`.",
				request.turn_id
			),
		));
	}

	None
}

fn lane_steer_request_rejection(
	run_request: &AppServerRunRequest<'_>,
	request: &LaneControlSteerRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Option<(&'static str, String)> {
	if request.project_id != run_request.project_id {
		return Some((
			"project_mismatch",
			format!(
				"Control request targeted project `{}`, but this run belongs to `{}`.",
				request.project_id, run_request.project_id
			),
		));
	}
	if request.issue_id != run_request.issue_id {
		return Some((
			"issue_mismatch",
			format!(
				"Control request targeted issue `{}`, but this run belongs to `{}`.",
				request.issue_id, run_request.issue_id
			),
		));
	}
	if request.run_id != run_request.run_id {
		return Some((
			"run_mismatch",
			format!(
				"Control request targeted run `{}`, but this run is `{}`.",
				request.run_id, run_request.run_id
			),
		));
	}
	if request.attempt_number != run_request.attempt_number {
		return Some((
			"attempt_mismatch",
			format!(
				"Control request targeted attempt `{}`, but this run is attempt `{}`.",
				request.attempt_number, run_request.attempt_number
			),
		));
	}
	if request.thread_id != target_thread_id {
		return Some((
			"thread_mismatch",
			format!(
				"Control request targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				request.thread_id
			),
		));
	}
	if request.expected_turn_id != target_turn_id {
		return Some((
			"stale_expected_turn_id",
			format!(
				"Control request expected turn `{}`, but the active turn is `{target_turn_id}`.",
				request.expected_turn_id
			),
		));
	}

	None
}

fn soft_interrupt_error_class(error: &Report) -> &'static str {
	if is_app_server_output_timeout(error) {
		return "soft_interrupt_timed_out";
	}

	let error_text = error.to_string().to_ascii_lowercase();

	if error_text.contains("-32601") || error_text.contains("method not found") {
		"soft_interrupt_unsupported"
	} else {
		"soft_interrupt_failed"
	}
}

pub(super) fn steer_error_class(error: &Report) -> &'static str {
	if is_app_server_output_timeout(error) {
		return "app_server_turn_steer_timed_out";
	}

	let error_text = error.to_string().to_ascii_lowercase();

	if error_text.contains("activeturnnotsteerable")
		|| error_text.contains("active turn not steerable")
	{
		return "active_turn_not_steerable";
	}
	if error_text.contains("-32601") || error_text.contains("method not found") {
		return "app_server_turn_steer_unsupported";
	}

	"app_server_turn_steer_failed"
}
