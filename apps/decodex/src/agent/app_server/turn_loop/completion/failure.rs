use crate::{
	agent::{
		app_server::{
			protocol::{ErrorNotification, TurnError},
			turn_failure::AppServerTurnFailure,
		},
		json_rpc::{JsonRpcError, JsonRpcNotification},
	},
	prelude::Result,
};

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

pub(in crate::agent::app_server::turn_loop::completion) fn turn_failure_from_turn_error(
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
