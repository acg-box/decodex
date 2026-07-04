mod completion;
mod messages;
mod resolution;
mod wait;

#[cfg(test)]
pub(in crate::agent::app_server) use self::{
	completion::{
		failure_from_error_notification, handle_turn_execution_notification,
		turn_failure_from_json_rpc_error_response,
	},
	messages::remaining_idle_budget,
	resolution::continuation_boundary_reached,
};
pub(in crate::agent::app_server) use self::{
	messages::{message_type, targets_thread, thread_id_from_value, turn_id_from_value},
	wait::{flush_pending_messages, is_app_server_output_timeout},
};

use crate::{
	agent::{
		app_server::{
			phase_goal::{self, PhaseGoalRunStatus},
			protocol::{
				AppServerClient, RunOutcome, ThreadGoalStatus, TurnStartRequest, TurnSteerRequest,
				UserInput,
			},
			runtime_types::{
				AppServerRunRequest, RequestDispatchContext, RequestWaitPhase, RunRecorder,
				TurnLoopResult,
			},
			server_requests::{self},
			transport,
		},
		codex_accounts::CodexAccountProvider,
		tracker_tool_bridge::DynamicToolHandler,
	},
	prelude::Result,
	state::StateStore,
};

pub(super) fn execute_turn_loop<'run>(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'run>,
	request: &'run AppServerRunRequest<'run>,
	state_store: &StateStore,
	thread_id: &str,
) -> Result<TurnLoopResult> {
	let mut next_input = request.user_input.clone();
	let mut turn_count = 0_u32;
	let mut phase_goal_runtime =
		phase_goal::initialize_phase_goal_runtime(client, recorder, request, thread_id)?;
	let mut phase_goal_status = phase_goal_runtime.as_ref().map(|runtime| PhaseGoalRunStatus {
		phase: runtime.active_goal.phase,
		status: ThreadGoalStatus::Active.as_str().to_owned(),
	});

	loop {
		let turn_id = start_turn_for_run(
			client,
			recorder,
			request.dynamic_tool_handler,
			request.codex_account_provider,
			thread_id,
			&next_input,
		)?;

		turn_count = turn_count.saturating_add(1);

		state_store.update_run_turn(&request.run_id, &turn_id)?;
		recorder.set_turn_id(&turn_id)?;

		self::wait::flush_pending_messages(client, recorder, Some(thread_id))?;

		let outcome =
			wait::wait_for_turn_completion(client, recorder, request, thread_id, &turn_id)?;
		let final_turn_id = outcome.turn_id;
		let final_output = outcome.final_output;

		if let Some((continuation_pending, observed_phase_goal_status)) =
			resolution::resolve_turn_completion(
				client,
				recorder,
				request,
				&mut phase_goal_runtime,
				thread_id,
				turn_count,
				&final_output,
			)? {
			if observed_phase_goal_status.is_some() {
				phase_goal_status = observed_phase_goal_status;
			}

			return Ok(TurnLoopResult {
				turn_id: final_turn_id,
				turn_count,
				final_output,
				continuation_pending,
				phase_goal_status,
			});
		}

		phase_goal_status = phase_goal_runtime.as_ref().map(|runtime| PhaseGoalRunStatus {
			phase: runtime.active_goal.phase,
			status: ThreadGoalStatus::Active.as_str().to_owned(),
		});
		next_input =
			request.continuation_user_input.clone().unwrap_or_else(|| request.user_input.clone());
	}
}

pub(super) fn build_turn_start_request(thread_id: &str, user_input: &str) -> TurnStartRequest {
	TurnStartRequest {
		thread_id: thread_id.to_owned(),
		input: vec![UserInput::Text { text: user_input.to_owned() }],
		..TurnStartRequest::default()
	}
}

pub(super) fn build_turn_steer_request(
	thread_id: &str,
	expected_turn_id: &str,
	message: &str,
) -> TurnSteerRequest {
	TurnSteerRequest {
		thread_id: thread_id.to_owned(),
		expected_turn_id: expected_turn_id.to_owned(),
		input: vec![UserInput::Text { text: message.to_owned() }],
	}
}

fn start_turn_for_run(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	codex_account_provider: Option<&dyn CodexAccountProvider>,
	thread_id: &str,
	next_input: &str,
) -> Result<String> {
	let turn_response = transport::annotate_transport_failure_phase(
		client.start_turn_with_handler(
			build_turn_start_request(thread_id, next_input),
			|connection, wire_message, server_request| {
				server_requests::handle_server_request_while_waiting(
					connection,
					recorder,
					wire_message,
					server_request,
					RequestDispatchContext::new(
						RequestWaitPhase::TurnStart,
						dynamic_tool_handler,
						codex_account_provider,
						Some(thread_id),
						None,
					),
				)
			},
		),
		RequestWaitPhase::TurnStart,
	)?;

	Ok(turn_response.turn.id)
}
