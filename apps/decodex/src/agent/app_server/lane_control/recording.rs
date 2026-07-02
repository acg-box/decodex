use crate::{
	agent::app_server::{
		LaneControlInterruptRequest, LaneControlInterruptResponse, LaneControlSteerRequest,
		LaneControlSteerResponse, LaneControlSteerResponseStatus, RUN_CONTROL_ACTION_COMPLETED,
		RUN_CONTROL_ACTION_FAILED, RunControlActionOutcomeRequest, RunRecorder, serde_json,
	},
	prelude::Result,
};

pub(in crate::agent::app_server::lane_control) fn record_lane_interrupt_request(
	recorder: &mut RunRecorder<'_>,
	request: &LaneControlInterruptRequest,
) -> Result<()> {
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

pub(in crate::agent::app_server::lane_control) fn record_lane_interrupt_response(
	recorder: &mut RunRecorder<'_>,
	response: &LaneControlInterruptResponse,
) -> Result<()> {
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

pub(in crate::agent::app_server::lane_control) fn record_lane_steer_request(
	recorder: &mut RunRecorder<'_>,
	request: &LaneControlSteerRequest,
) -> Result<()> {
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

pub(in crate::agent::app_server::lane_control) fn record_lane_steer_response(
	recorder: &mut RunRecorder<'_>,
	response: &LaneControlSteerResponse,
	parent_record_id: Option<i64>,
) -> Result<()> {
	let outcome = match &response.status {
		LaneControlSteerResponseStatus::Delivered => RUN_CONTROL_ACTION_COMPLETED,
		LaneControlSteerResponseStatus::Failed | LaneControlSteerResponseStatus::Rejected =>
			RUN_CONTROL_ACTION_FAILED,
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
