mod adoption;
mod failure;
mod notification;

pub(in crate::agent::app_server::turn_loop) use self::adoption::adopt_thread_bound_notification_turn_id;

use crate::{
	agent::{
		app_server::{protocol::RunOutcome, turn_failure::AppServerTurnFailure},
		json_rpc::{JsonRpcError, JsonRpcNotification},
	},
	prelude::Result,
};

#[cfg(test)]
pub(in crate::agent::app_server) fn failure_from_error_notification(
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Result<Option<(AppServerTurnFailure, Option<bool>)>> {
	failure::failure_from_error_notification(notification, target_thread_id, target_turn_id)
}

pub(in crate::agent::app_server) fn handle_turn_execution_notification(
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &str,
	final_output: &mut String,
	latest_turn_failure: &mut Option<AppServerTurnFailure>,
) -> Result<Option<RunOutcome>> {
	notification::handle_turn_execution_notification(
		notification,
		target_thread_id,
		target_turn_id,
		final_output,
		latest_turn_failure,
	)
}

pub(in crate::agent::app_server) fn turn_failure_from_json_rpc_error_response(
	thread_id: &str,
	turn_id: &str,
	error: &JsonRpcError,
) -> AppServerTurnFailure {
	failure::turn_failure_from_json_rpc_error_response(thread_id, turn_id, error)
}
