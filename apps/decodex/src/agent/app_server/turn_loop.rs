use std::{
	mem,
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde_json::Value;

use super::{
	activity::protocol_activity_idle_timeout,
	constants::RUN_CONTROL_POLL_INTERVAL,
	dynamic_tools::{
		classify_turn_completion, has_terminal_completion_signal,
		reject_nonterminal_single_turn_completion,
	},
	lane_control::handle_pending_turn_control_requests,
	phase_goal::{
		AppServerPhaseGoalFailure, PhaseGoalRunStatus, PhaseGoalRuntime, PhaseGoalTransition,
		app_server_method_not_found, clear_thread_phase_goal_best_effort, get_thread_phase_goal,
		initialize_phase_goal_runtime, record_phase_goal_completed, set_thread_phase_goal,
	},
	protocol::{
		AgentMessageDeltaNotification, AppServerClient, ErrorNotification,
		ItemCompletedNotification, RunOutcome, ThreadGoalStatus, ThreadGoalUpdatedNotification,
		ThreadStatusChangedNotification, TurnCompletedNotification, TurnError, TurnStartRequest,
		TurnSteerRequest, UserInput,
	},
	runtime_types::{
		AppServerRunRequest, RequestDispatchContext, RequestWaitPhase, RunRecorder,
		TurnContinuationGuard, TurnLoopResult,
	},
	server_requests::{
		apply_protocol_message_side_effects, handle_server_request_during_turn_execution,
		handle_server_request_while_waiting,
	},
	transport::annotate_transport_failure_phase,
	turn_failure::AppServerTurnFailure,
};
use crate::{
	agent::{
		codex_accounts::CodexAccountProvider,
		json_rpc::{
			AppServerOutputTimeout, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
			JsonRpcRequest, WireMessage,
		},
		tracker_tool_bridge::{DynamicToolHandler, TurnCompletionStatus},
	},
	prelude::eyre,
	state::StateStore,
};

pub(super) fn execute_turn_loop(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
	thread_id: &str,
) -> crate::prelude::Result<TurnLoopResult> {
	let mut next_input = request.user_input.clone();
	let mut turn_count = 0_u32;
	let mut phase_goal_runtime =
		initialize_phase_goal_runtime(client, recorder, request, thread_id)?;
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

		flush_pending_messages(client, recorder, Some(thread_id))?;

		let outcome = wait_for_turn_completion(client, recorder, request, thread_id, &turn_id)?;
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

fn start_turn_for_run(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	codex_account_provider: Option<&dyn CodexAccountProvider>,
	thread_id: &str,
	next_input: &str,
) -> crate::prelude::Result<String> {
	let turn_response = annotate_transport_failure_phase(
		client.start_turn_with_handler(
			build_turn_start_request(thread_id, next_input),
			|connection, wire_message, server_request| {
				handle_server_request_while_waiting(
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
) -> crate::prelude::Result<Option<(bool, Option<PhaseGoalRunStatus>)>> {
	let completion_status = classify_turn_completion(request.dynamic_tool_handler, final_output)?;
	let terminal_completion_signal = has_terminal_completion_signal(request.dynamic_tool_handler);

	if phase_goal_runtime.is_some() {
		let observed_goal_result = {
			let runtime = phase_goal_runtime
				.as_ref()
				.expect("phase goal runtime should be present after is_some check");

			get_thread_phase_goal(client, recorder, thread_id, runtime)
		};
		let observed_goal = match observed_goal_result {
			Ok(goal) => goal,
			Err(error) if app_server_method_not_found(&error) => {
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

			record_phase_goal_completed(recorder, runtime.active_goal.phase, &observed_goal)?;

			match transition {
				PhaseGoalTransition::Continue(next_goal) => {
					if completion_status == TurnCompletionStatus::Complete
						&& terminal_completion_signal
					{
						return Ok(Some((false, Some(observed_status))));
					}

					set_thread_phase_goal(client, recorder, thread_id, &next_goal)?;

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
						clear_thread_phase_goal_best_effort(client, recorder, thread_id);

						return Ok(Some((false, Some(observed_status))));
					}

					return Err(Report::new(AppServerPhaseGoalFailure::missing_terminal_path(
						runtime.active_goal.phase,
					)));
				},
			}
		}
		if completion_status == TurnCompletionStatus::Complete && terminal_completion_signal {
			clear_thread_phase_goal_best_effort(client, recorder, thread_id);

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
) -> crate::prelude::Result<Option<bool>> {
	match completion_status {
		TurnCompletionStatus::Complete => Ok(Some(false)),
		TurnCompletionStatus::Continue => {
			if request.max_turns <= 1 {
				reject_nonterminal_single_turn_completion(
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

pub(super) fn continuation_boundary_reached(
	continuation_guard: Option<&dyn TurnContinuationGuard>,
	turn_count: u32,
) -> crate::prelude::Result<bool> {
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

pub(super) fn flush_pending_messages(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	target_thread_id: Option<&str>,
) -> crate::prelude::Result<()> {
	for message in client.drain_pending() {
		if targets_thread(&message, target_thread_id) {
			recorder.record(message_type(&message), &message.raw)?;

			apply_protocol_message_side_effects(recorder, &message)?;
		}
	}

	Ok(())
}

fn wait_for_turn_completion(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<RunOutcome> {
	let control_enabled = request.activity_marker_path.is_some();
	let mut last_activity_at = Instant::now();
	let mut target_turn_id = target_turn_id.to_owned();
	let mut final_output = String::new();
	let mut latest_turn_failure: Option<AppServerTurnFailure> = None;

	loop {
		if control_enabled
			&& let Some(response_turn_id) = handle_pending_turn_control_requests(
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

		let idle_timeout = protocol_activity_idle_timeout(
			Some(&recorder.protocol_activity.summary),
			request.timeout,
		);
		let Some(wire_message) = next_turn_wire_message(
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

		if !targets_thread(&wire_message, Some(target_thread_id)) {
			tracing::debug!(raw = %wire_message.raw, "Ignoring app-server message for another thread.");

			continue;
		}

		last_activity_at = Instant::now();

		recorder.record(message_type(&wire_message), &wire_message.raw)?;

		apply_protocol_message_side_effects(recorder, &wire_message)?;

		match &wire_message.message {
			JsonRpcMessage::Notification(notification) => {
				adopt_thread_bound_notification_turn_id(
					recorder,
					notification,
					target_thread_id,
					&mut target_turn_id,
				)?;

				if let Some(outcome) = handle_turn_execution_notification(
					notification,
					target_thread_id,
					&target_turn_id,
					&mut final_output,
					&mut latest_turn_failure,
				)? {
					return Ok(outcome);
				}
			},
			JsonRpcMessage::Request(server_request) => handle_turn_execution_request(
				client,
				recorder,
				server_request,
				target_thread_id,
				&target_turn_id,
				request.dynamic_tool_handler,
				request.codex_account_provider,
			)?,
			JsonRpcMessage::Response(_) => ignore_orphan_turn_json_rpc_response(),
			JsonRpcMessage::Error(error) => {
				latest_turn_failure = Some(turn_failure_from_json_rpc_error_response(
					target_thread_id,
					&target_turn_id,
					error,
				));
			},
		}
	}
}

pub(super) fn handle_turn_execution_notification(
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &str,
	final_output: &mut String,
	latest_turn_failure: &mut Option<AppServerTurnFailure>,
) -> crate::prelude::Result<Option<RunOutcome>> {
	match notification.method.as_str() {
		"thread/status/changed" => {
			let payload: ThreadStatusChangedNotification =
				serde_json::from_value(notification.params.clone())?;

			if payload.status.kind == "systemError" && latest_turn_failure.is_none() {
				*latest_turn_failure =
					Some(AppServerTurnFailure::from_system_error(&payload.thread_id));
			}
		},
		"error" => {
			if let Some((failure, will_retry)) =
				failure_from_error_notification(notification, target_thread_id, target_turn_id)?
			{
				if (failure.requires_operator_attention() || failure.should_stop_current_turn())
					&& will_retry != Some(true)
				{
					return Err(Report::new(failure));
				}

				*latest_turn_failure = Some(failure);
			}
		},
		"item/agentMessage/delta" => {
			if !notification_targets_turn(notification, target_turn_id) {
				return Ok(None);
			}

			let payload: AgentMessageDeltaNotification =
				serde_json::from_value(notification.params.clone())?;

			final_output.push_str(&payload.delta);
		},
		"item/completed" => {
			if !notification_targets_turn(notification, target_turn_id) {
				return Ok(None);
			}

			let payload: ItemCompletedNotification =
				serde_json::from_value(notification.params.clone())?;

			if payload.item.kind == "agentMessage"
				&& let Some(text) = payload.item.text
			{
				*final_output = text;
			}
		},
		"turn/completed" => {
			let payload: TurnCompletedNotification =
				serde_json::from_value(notification.params.clone())?;

			if payload.turn.id != target_turn_id {
				return Ok(None);
			}
			if payload.turn.status == "completed" {
				return Ok(Some(RunOutcome {
					final_output: mem::take(final_output),
					turn_id: target_turn_id.to_owned(),
				}));
			}

			if let Some(error) = payload.turn.error.as_ref() {
				return Err(Report::new(turn_failure_from_turn_error(
					target_thread_id,
					Some(&payload.turn.id),
					&payload.turn.status,
					error,
				)));
			}
			if let Some(failure) = latest_turn_failure.take() {
				return Err(Report::new(failure));
			}

			eyre::bail!(
				"Turn `{}` ended with status `{}` without an explicit error payload.",
				payload.turn.id,
				payload.turn.status
			);
		},
		"thread/goal/updated" => {
			let payload: ThreadGoalUpdatedNotification =
				serde_json::from_value(notification.params.clone())?;

			if payload.thread_id != target_thread_id
				|| payload.turn_id.as_deref().is_some_and(|turn_id| turn_id != target_turn_id)
			{
				return Ok(None);
			}

			let _status = payload.goal.status;
		},
		"thread/goal/cleared" => {},
		_ => {},
	}

	Ok(None)
}

fn adopt_thread_bound_notification_turn_id(
	recorder: &mut RunRecorder<'_>,
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &mut String,
) -> crate::prelude::Result<()> {
	let Some(observed_turn_id) = turn_id_from_value(&notification.params) else {
		return Ok(());
	};

	if observed_turn_id == target_turn_id {
		return Ok(());
	}
	if thread_id_from_notification(notification)
		.is_none_or(|thread_id| thread_id != target_thread_id)
	{
		return Ok(());
	}

	tracing::warn!(
		target_thread_id,
		previous_turn_id = target_turn_id.as_str(),
		observed_turn_id,
		method = notification.method.as_str(),
		"App-server notification turn id differed from the turn/start response; adopting thread-bound notification turn id."
	);

	recorder.state_store.update_run_turn(recorder.run_id, observed_turn_id)?;
	recorder.set_turn_id(observed_turn_id)?;

	*target_turn_id = observed_turn_id.to_owned();

	Ok(())
}

fn notification_targets_turn(notification: &JsonRpcNotification, target_turn_id: &str) -> bool {
	turn_id_from_value(&notification.params).is_none_or(|turn_id| turn_id == target_turn_id)
}

fn next_turn_wire_message(
	client: &mut AppServerClient,
	last_activity_at: Instant,
	timeout: Duration,
	target_thread_id: &str,
	target_turn_id: &str,
	latest_turn_failure: Option<&AppServerTurnFailure>,
	control_enabled: bool,
) -> crate::prelude::Result<Option<WireMessage>> {
	let now = Instant::now();
	let wait_timeout = remaining_idle_budget(last_activity_at, now, timeout).ok_or_else(|| {
		turn_wait_timeout_error(target_thread_id, target_turn_id, latest_turn_failure.cloned())
	})?;
	let recv_timeout =
		if control_enabled { wait_timeout.min(RUN_CONTROL_POLL_INTERVAL) } else { wait_timeout };

	match recv_turn_wire_message(client, recv_timeout, latest_turn_failure) {
		Ok(wire_message) => Ok(Some(wire_message)),
		Err(error)
			if control_enabled
				&& recv_timeout < wait_timeout
				&& is_app_server_output_timeout(&error) =>
		{
			Ok(None)
		},
		Err(error) => Err(error),
	}
}

pub(super) fn is_app_server_output_timeout(error: &Report) -> bool {
	error.downcast_ref::<AppServerOutputTimeout>().is_some()
}

fn handle_turn_execution_request(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: &str,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	codex_account_provider: Option<&dyn CodexAccountProvider>,
) -> crate::prelude::Result<()> {
	handle_server_request_during_turn_execution(
		client,
		recorder,
		request,
		RequestDispatchContext::new(
			RequestWaitPhase::TurnExecution,
			dynamic_tool_handler,
			codex_account_provider,
			Some(target_thread_id),
			Some(target_turn_id),
		),
	)
}

fn ignore_orphan_turn_json_rpc_response() {
	tracing::debug!(
		"Recorded and ignored orphan app-server JSON-RPC response while waiting for turn completion."
	);
}

fn turn_wait_timeout_error(
	target_thread_id: &str,
	target_turn_id: &str,
	latest_turn_failure: Option<AppServerTurnFailure>,
) -> Report {
	let message = format!(
		"Timed out while waiting for turn `{target_turn_id}` on thread `{target_thread_id}`."
	);

	if let Some(failure) = latest_turn_failure {
		return Report::new(failure).wrap_err(message);
	}

	eyre::eyre!(message)
}

fn recv_turn_wire_message(
	client: &mut AppServerClient,
	wait_timeout: Duration,
	latest_turn_failure: Option<&AppServerTurnFailure>,
) -> crate::prelude::Result<WireMessage> {
	match annotate_transport_failure_phase(
		client.recv(Some(wait_timeout)),
		RequestWaitPhase::TurnExecution,
	) {
		Ok(wire_message) => Ok(wire_message),
		Err(error) => {
			if error.downcast_ref::<AppServerOutputTimeout>().is_some()
				&& let Some(failure) = latest_turn_failure
			{
				return Err(Report::new(failure.clone())
					.wrap_err("Timed out while waiting for additional app-server output."));
			}

			Err(error)
		},
	}
}

pub(super) fn failure_from_error_notification(
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<Option<(AppServerTurnFailure, Option<bool>)>> {
	let payload: ErrorNotification = serde_json::from_value(notification.params.clone())?;
	let payload_turn_matches =
		payload.turn_id.as_deref().is_none_or(|turn_id| turn_id == target_turn_id);
	let payload_thread_matches =
		payload.thread_id.as_deref().is_none_or(|thread_id| thread_id == target_thread_id);

	if !payload_thread_matches || !payload_turn_matches {
		return Ok(None);
	}

	let failure = turn_failure_from_turn_error(
		target_thread_id,
		payload.turn_id.as_deref(),
		"failed",
		&payload.error,
	);

	Ok(Some((failure, payload.will_retry)))
}

fn turn_failure_from_turn_error(
	thread_id: &str,
	turn_id: Option<&str>,
	status: &str,
	error: &TurnError,
) -> AppServerTurnFailure {
	AppServerTurnFailure::new(
		thread_id,
		turn_id.map(str::to_owned),
		status,
		error.message.clone(),
		error.codex_error_info.clone(),
	)
}

pub(super) fn turn_failure_from_json_rpc_error_response(
	thread_id: &str,
	turn_id: &str,
	error: &JsonRpcError,
) -> AppServerTurnFailure {
	tracing::warn!(
		id = %error.id,
		code = error.error.code,
		message = %error.error.message,
		"Received JSON-RPC error response while waiting for turn completion."
	);

	AppServerTurnFailure::new(
		thread_id,
		Some(turn_id.to_owned()),
		"failed",
		format!(
			"app-server JSON-RPC error response while waiting for turn completion: code {}: {}",
			error.error.code, error.error.message
		),
		None,
	)
}

pub(super) fn remaining_idle_budget(
	last_activity_at: Instant,
	now: Instant,
	timeout: Duration,
) -> Option<Duration> {
	timeout.checked_sub(now.saturating_duration_since(last_activity_at))
}

pub(super) fn message_type(message: &WireMessage) -> &str {
	match &message.message {
		JsonRpcMessage::Notification(notification) => notification.method.as_str(),
		JsonRpcMessage::Request(request) => request.method.as_str(),
		JsonRpcMessage::Response(_) => "json-rpc/response",
		JsonRpcMessage::Error(_) => "json-rpc/error",
	}
}

pub(super) fn targets_thread(message: &WireMessage, target_thread_id: Option<&str>) -> bool {
	let Some(target_thread_id) = target_thread_id else {
		return true;
	};

	match &message.message {
		JsonRpcMessage::Notification(notification) => thread_id_from_notification(notification)
			.is_none_or(|thread_id| thread_id == target_thread_id),
		JsonRpcMessage::Request(request) => thread_id_from_value(&request.params)
			.is_none_or(|thread_id| thread_id == target_thread_id),
		JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => true,
	}
}

fn thread_id_from_notification(notification: &JsonRpcNotification) -> Option<&str> {
	thread_id_from_value(&notification.params)
}

pub(super) fn thread_id_from_value(value: &Value) -> Option<&str> {
	value
		.get("threadId")
		.and_then(Value::as_str)
		.or_else(|| value.get("thread").and_then(|thread| thread.get("id")).and_then(Value::as_str))
}

pub(super) fn turn_id_from_value(value: &Value) -> Option<&str> {
	value
		.get("turnId")
		.and_then(Value::as_str)
		.or_else(|| value.get("turn").and_then(|turn| turn.get("id")).and_then(Value::as_str))
}
