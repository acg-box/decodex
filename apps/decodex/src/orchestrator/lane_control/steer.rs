mod report;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		LaneSteerReport, LaneSteerRequest, OperatorRunStatus,
		lane_control::context::{self},
	},
	prelude::{Result, eyre},
	run_control::{
		self, LaneControlSteerRequest, LaneControlSteerRequestInput, LaneControlSteerResponseStatus,
	},
	state::{
		RUN_CONTROL_ACTION_ACCEPTED, RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED,
		RUN_CONTROL_ACTION_TIMED_OUT, RunControlActionRequest, StateStore,
	},
};

pub(super) fn attempt_lane_steer(
	state_store: &StateStore,
	project: &ServiceConfig,
	run: &OperatorRunStatus,
	request: &LaneSteerRequest<'_>,
) -> Result<LaneSteerReport> {
	let message_byte_count = request.message.len();
	let message_line_count = report::lane_steer_message_line_count(request.message);
	let context = context::lane_control_operator_context(run);
	let metadata = serde_json::json!({
		"expectedTurnId": request.expected_turn_id,
		"messageByteCount": message_byte_count,
		"messageLineCount": message_line_count,
	});
	let receipt = state_store.resolve_run_control_action(RunControlActionRequest {
		project_id: project.service_id(),
		issue_id: &run.issue_id,
		run_id: &run.run_id,
		attempt_number: run.attempt_number,
		thread_id: run.thread_id.as_deref(),
		turn_id: Some(request.expected_turn_id),
		source: request.source,
		action: "steer",
		timeout_ms: Some(i64::try_from(request.wait_timeout.as_millis()).unwrap_or(i64::MAX)),
		metadata: Some(&metadata),
		context: Some(&context),
	})?;

	if receipt.outcome() != RUN_CONTROL_ACTION_ACCEPTED {
		return Ok(report::lane_steer_report_from_rejected_receipt(
			request.issue,
			run,
			&receipt,
			request.expected_turn_id,
			message_byte_count,
			message_line_count,
		));
	}

	let Some(worktree_path) = context::absolute_lane_worktree_path(project, state_store, run)?
	else {
		eyre::bail!("Lane steer was accepted without a current lane worktree.");
	};
	let Some(thread_id) = run.thread_id.as_deref() else {
		eyre::bail!("Lane steer was accepted before the active app-server thread id was known.");
	};
	let control_request = LaneControlSteerRequest::new(LaneControlSteerRequestInput {
		audit_record_id: receipt.audit_record_id(),
		project_id: project.service_id(),
		issue_id: &run.issue_id,
		run_id: &run.run_id,
		attempt_number: run.attempt_number,
		thread_id,
		expected_turn_id: request.expected_turn_id,
		source: request.source,
		message: request.message,
	});
	let request_path = run_control::write_steer_request(&worktree_path, &control_request)?;

	state_store.append_private_execution_event(
		project.service_id(),
		&run.issue_id,
		&run.run_id,
		run.attempt_number,
		"lane_control/steer/requested",
		serde_json::json!({
			"requestId": control_request.request_id,
			"source": request.source,
			"method": "turn/steer",
			"expectedTurnId": request.expected_turn_id,
			"messageByteCount": control_request.message_byte_count,
			"messageLineCount": control_request.message_line_count,
		}),
	)?;

	match run_control::wait_for_steer_response(
		&worktree_path,
		&run.run_id,
		&control_request.request_id,
		request.wait_timeout,
	)? {
		Some(response) => {
			let outcome = match &response.status {
				LaneControlSteerResponseStatus::Delivered => RUN_CONTROL_ACTION_COMPLETED,
				LaneControlSteerResponseStatus::Failed
				| LaneControlSteerResponseStatus::Rejected => RUN_CONTROL_ACTION_FAILED,
			};

			state_store.record_run_control_action_outcome(
				&receipt,
				outcome,
				&response.classification,
			)?;

			Ok(report::lane_steer_report_from_response(
				request.issue,
				run,
				&receipt,
				&control_request,
				&request_path,
				response,
			))
		},
		None => {
			state_store.record_run_control_action_outcome(
				&receipt,
				RUN_CONTROL_ACTION_TIMED_OUT,
				"steer_response_pending",
			)?;

			Ok(report::lane_steer_report_pending(
				request.issue,
				run,
				&receipt,
				&control_request,
				&request_path,
			))
		},
	}
}

pub(super) fn validate_lane_steer_request(request: &LaneSteerRequest<'_>) -> Result<()> {
	if request.issue.trim().is_empty() {
		eyre::bail!("Lane steer issue must not be empty.");
	}
	if request.run_id.trim().is_empty() {
		eyre::bail!("Lane steer run id must not be empty.");
	}
	if request.expected_turn_id.trim().is_empty() {
		eyre::bail!("Lane steer expected turn id must not be empty.");
	}
	if request.message.trim().is_empty() {
		eyre::bail!("Lane steer message must not be empty.");
	}
	if request.source.trim().is_empty() {
		eyre::bail!("Lane steer source must not be empty.");
	}

	Ok(())
}
