use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, OperatorRunStatus,
		lane_control::{
			constants::LANE_INTERRUPT_RESPONSE_WAIT, context, reports::LaneSoftInterruptReport,
		},
	},
	prelude::Result,
	run_control::{
		self, LaneControlInterruptRequest, LaneControlInterruptRequestInput,
		LaneControlResponseStatus,
	},
	state::{
		RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED, RUN_CONTROL_ACTION_TIMED_OUT,
		RunControlActionReceipt, RunControlActionRequest, StateStore,
	},
};

pub(in crate::orchestrator::lane_control) fn attempt_soft_lane_interrupt(
	state_store: &StateStore,
	project: &ServiceConfig,
	run: &OperatorRunStatus,
	force: bool,
	reason: Option<&str>,
	source: &str,
) -> Result<LaneSoftInterruptReport> {
	let Some(worktree_path) = context::absolute_lane_worktree_path(project, state_store, run)?
	else {
		return Ok(LaneSoftInterruptReport::unavailable(
			"worktree_missing",
			"Soft interrupt requires the current lane worktree and run-control directory.",
		));
	};
	let Some(thread_id) = run.thread_id.as_deref() else {
		return Ok(LaneSoftInterruptReport::unavailable(
			"thread_id_missing",
			"Soft interrupt requires a recorded app-server thread id.",
		));
	};
	let Some(turn_id) = run.turn_id.as_deref() else {
		return Ok(LaneSoftInterruptReport::unavailable(
			"turn_id_missing",
			"Soft interrupt requires a recorded active app-server turn id.",
		));
	};

	if !orchestrator::operator_run_counts_as_current_lane(run) {
		return Ok(LaneSoftInterruptReport::unavailable(
			"lane_not_active",
			"Soft interrupt only targets current or live local lane runs.",
		));
	}

	let receipt = resolve_soft_interrupt_control_action(
		state_store,
		project,
		run,
		thread_id,
		turn_id,
		source,
	)?;

	if receipt.outcome() != "accepted" {
		return Ok(LaneSoftInterruptReport::from_control_rejection(&receipt));
	}

	let request = LaneControlInterruptRequest::new(LaneControlInterruptRequestInput {
		project_id: project.service_id(),
		issue_id: &run.issue_id,
		run_id: &run.run_id,
		attempt_number: run.attempt_number,
		thread_id,
		turn_id,
		source,
		reason,
	});

	run_control::write_interrupt_request(&worktree_path, &request)?;

	state_store.append_private_execution_event(
		project.service_id(),
		&run.issue_id,
		&run.run_id,
		run.attempt_number,
		"lane_control/interrupt/requested",
		serde_json::json!({
			"requestId": request.request_id,
			"source": source,
			"method": "turn/interrupt",
			"force": force,
			"reason": reason,
		}),
	)?;

	match run_control::wait_for_interrupt_response(
		&worktree_path,
		&run.run_id,
		&request.request_id,
		LANE_INTERRUPT_RESPONSE_WAIT,
	)? {
		Some(response) => {
			let outcome = match &response.status {
				LaneControlResponseStatus::SoftDelivered => RUN_CONTROL_ACTION_COMPLETED,
				LaneControlResponseStatus::SoftFailed => RUN_CONTROL_ACTION_FAILED,
				LaneControlResponseStatus::Rejected => RUN_CONTROL_ACTION_FAILED,
			};

			state_store.record_run_control_action_outcome(
				&receipt,
				outcome,
				&response.classification,
			)?;

			Ok(LaneSoftInterruptReport::from_response(response))
		},
		None => {
			state_store.record_run_control_action_outcome(
				&receipt,
				RUN_CONTROL_ACTION_TIMED_OUT,
				"soft_interrupt_response_pending",
			)?;

			Ok(LaneSoftInterruptReport {
				attempted: true,
				available: true,
				status: String::from("pending"),
				classification: String::from("soft_interrupt_pending"),
				method: String::from("turn/interrupt"),
				request_id: Some(request.request_id),
				message: String::from(
					"Soft interrupt request was written, but the app-server child has not recorded a response yet.",
				),
				error_class: Some(String::from("soft_interrupt_response_pending")),
				protocol_summary: None,
				response: None,
			})
		},
	}
}

fn resolve_soft_interrupt_control_action(
	state_store: &StateStore,
	project: &ServiceConfig,
	run: &OperatorRunStatus,
	thread_id: &str,
	turn_id: &str,
	source: &str,
) -> Result<RunControlActionReceipt> {
	let context = context::lane_control_operator_context(run);

	state_store.resolve_run_control_action(RunControlActionRequest {
		project_id: project.service_id(),
		issue_id: &run.issue_id,
		run_id: &run.run_id,
		attempt_number: run.attempt_number,
		thread_id: Some(thread_id),
		turn_id: Some(turn_id),
		source,
		action: "interrupt",
		timeout_ms: Some(
			i64::try_from(LANE_INTERRUPT_RESPONSE_WAIT.as_millis()).unwrap_or(i64::MAX),
		),
		metadata: None,
		context: Some(&context),
	})
}
