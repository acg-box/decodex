use std::mem;

use color_eyre::eyre::Report;

use crate::{
	agent::{
		app_server::{
			protocol::{
				AgentMessageDeltaNotification, ErrorNotification, ItemCompletedNotification,
				RunOutcome, ThreadGoalUpdatedNotification, ThreadStatusChangedNotification,
				TurnCompletedNotification, TurnError,
			},
			runtime_types::RunRecorder,
			turn_failure::AppServerTurnFailure,
			turn_loop::messages,
		},
		json_rpc::{JsonRpcError, JsonRpcNotification},
	},
	prelude::{Result, eyre},
};

pub(in crate::agent::app_server) fn handle_turn_execution_notification(
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &str,
	final_output: &mut String,
	latest_turn_failure: &mut Option<AppServerTurnFailure>,
) -> Result<Option<RunOutcome>> {
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
				if will_retry == Some(true) {
					return Ok(None);
				}
				if failure.requires_operator_attention() || failure.should_stop_current_turn() {
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

pub(in crate::agent::app_server) fn failure_from_error_notification(
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Result<Option<(AppServerTurnFailure, Option<bool>)>> {
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

pub(in crate::agent::app_server) fn turn_failure_from_json_rpc_error_response(
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

pub(super) fn adopt_thread_bound_notification_turn_id(
	recorder: &mut RunRecorder<'_>,
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &mut String,
) -> Result<()> {
	let Some(observed_turn_id) = messages::turn_id_from_value(&notification.params) else {
		return Ok(());
	};

	if observed_turn_id == target_turn_id {
		return Ok(());
	}
	if messages::thread_id_from_notification(notification)
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
	messages::turn_id_from_value(&notification.params)
		.is_none_or(|turn_id| turn_id == target_turn_id)
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
