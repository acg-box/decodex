mod completion;
mod messages;
mod wait;

#[cfg(test)]
pub(in crate::agent::app_server) use self::{
	completion::{
		failure_from_error_notification, handle_turn_execution_notification,
		turn_failure_from_json_rpc_error_response,
	},
	messages::remaining_idle_budget,
};
pub(in crate::agent::app_server) use self::{
	messages::{message_type, targets_thread, thread_id_from_value, turn_id_from_value},
	wait::{flush_pending_messages, is_app_server_output_timeout},
};

use color_eyre::Report;

use crate::{
	agent::{
		app_server::{
			dynamic_tools::{self},
			phase_goal::{
				self, AppServerPhaseGoalFailure, PhaseGoalRunStatus, PhaseGoalRuntime,
				PhaseGoalTransition,
			},
			protocol::{
				AppServerClient, RunOutcome, ThreadGoalStatus, TurnStartRequest, TurnSteerRequest,
				UserInput,
			},
			runtime_types::{
				AppServerRunRequest, RequestDispatchContext, RequestWaitPhase, RunRecorder,
				TurnContinuationGuard, TurnLoopResult,
			},
			server_requests::{self},
			transport,
		},
		codex_accounts::CodexAccountProvider,
		tracker_tool_bridge::{DynamicToolHandler, TurnCompletionStatus},
	},
	prelude::Result,
	state::StateStore,
};

pub(super) fn execute_turn_loop(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
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

		if let Some((continuation_pending, observed_phase_goal_status)) = resolve_turn_completion(
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

pub(super) fn continuation_boundary_reached(
	continuation_guard: Option<&dyn TurnContinuationGuard>,
	turn_count: u32,
) -> Result<bool> {
	let Some(continuation_guard) = continuation_guard else {
		return Ok(false);
	};

	if continuation_guard.should_continue_turn(turn_count)? {
		return Ok(false);
	}

	continuation_guard.validate_continuation_boundary(turn_count)?;

	Ok(true)
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

fn resolve_turn_completion(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	phase_goal_runtime: &mut Option<PhaseGoalRuntime<'_>>,
	thread_id: &str,
	turn_count: u32,
	final_output: &str,
) -> Result<Option<(bool, Option<PhaseGoalRunStatus>)>> {
	let completion_status =
		dynamic_tools::classify_turn_completion(request.dynamic_tool_handler, final_output)?;
	let terminal_completion_signal =
		dynamic_tools::has_terminal_completion_signal(request.dynamic_tool_handler);

	if phase_goal_runtime.is_some() {
		let observed_goal_result = {
			let runtime = phase_goal_runtime
				.as_ref()
				.expect("phase goal runtime should be present after is_some check");

			phase_goal::get_thread_phase_goal(client, recorder, thread_id, runtime)
		};
		let observed_goal = match observed_goal_result {
			Ok(goal) => goal,
			Err(error) if phase_goal::app_server_method_not_found(&error) => {
				return Err(Report::new(AppServerPhaseGoalFailure::unsupported("thread/goal/get"))
					.wrap_err(error));
			},
			Err(error) => return Err(error),
		};
		let runtime = phase_goal_runtime
			.as_mut()
			.expect("phase goal runtime should still be present after goal status read");
		let observed_status = PhaseGoalRunStatus {
			phase: runtime.active_goal.phase,
			status: observed_goal.status.as_str().to_owned(),
		};

		if observed_goal.status == ThreadGoalStatus::Complete {
			let transition = runtime.controller.phase_goal_completed(runtime.active_goal.phase)?;

			phase_goal::record_phase_goal_completed(
				recorder,
				runtime.active_goal.phase,
				&observed_goal,
			)?;

			match transition {
				PhaseGoalTransition::Continue(next_goal) => {
					if completion_status == TurnCompletionStatus::Complete
						&& terminal_completion_signal
					{
						return Ok(Some((false, Some(observed_status))));
					}

					phase_goal::set_thread_phase_goal(client, recorder, thread_id, &next_goal)?;

					runtime.active_goal = next_goal;

					if turn_count >= request.max_turns {
						return Ok(Some((true, Some(observed_status))));
					}
					if continuation_boundary_reached(request.continuation_guard, turn_count)? {
						return Ok(Some((true, Some(observed_status))));
					}

					return Ok(None);
				},
				PhaseGoalTransition::CompleteRun => {
					if completion_status == TurnCompletionStatus::Complete
						&& terminal_completion_signal
					{
						phase_goal::clear_thread_phase_goal_best_effort(
							client, recorder, thread_id,
						);

						return Ok(Some((false, Some(observed_status))));
					}

					return Err(Report::new(AppServerPhaseGoalFailure::missing_terminal_path(
						runtime.active_goal.phase,
					)));
				},
			}
		}
		if completion_status == TurnCompletionStatus::Complete && terminal_completion_signal {
			phase_goal::clear_thread_phase_goal_best_effort(client, recorder, thread_id);

			return Ok(Some((false, Some(observed_status))));
		}
		if turn_count >= request.max_turns {
			return Ok(Some((true, Some(observed_status))));
		}
		if continuation_boundary_reached(request.continuation_guard, turn_count)? {
			return Ok(Some((true, Some(observed_status))));
		}

		return Ok(None);
	}

	resolve_turn_completion_without_phase_goal(request, turn_count, completion_status, final_output)
		.map(|result| result.map(|continuation_pending| (continuation_pending, None)))
}

fn resolve_turn_completion_without_phase_goal(
	request: &AppServerRunRequest<'_>,
	turn_count: u32,
	completion_status: TurnCompletionStatus,
	final_output: &str,
) -> Result<Option<bool>> {
	match completion_status {
		TurnCompletionStatus::Complete => Ok(Some(false)),
		TurnCompletionStatus::Continue => {
			if request.max_turns <= 1 {
				dynamic_tools::reject_nonterminal_single_turn_completion(
					request.dynamic_tool_handler,
					final_output,
				)?;
			}
			if turn_count >= request.max_turns {
				return Ok(Some(true));
			}
			if continuation_boundary_reached(request.continuation_guard, turn_count)? {
				return Ok(Some(true));
			}

			Ok(None)
		},
	}
}
