use std::mem;

use color_eyre::eyre::Report;

use crate::{
	agent::{
		app_server::{
			protocol::{
				AgentMessageDeltaNotification, ItemCompletedNotification, RunOutcome,
				ThreadGoalUpdatedNotification, ThreadStatusChangedNotification,
				TurnCompletedNotification,
			},
			turn_failure::AppServerTurnFailure,
			turn_loop::{completion::failure, messages},
		},
		json_rpc::JsonRpcNotification,
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

			if payload.status.kind == "systemError" {
				return Err(Report::new(latest_turn_failure.take().unwrap_or_else(|| {
					AppServerTurnFailure::from_system_error(&payload.thread_id)
				})));
			}
		},
		"error" => {
			if let Some((failure, will_retry)) = failure::failure_from_error_notification(
				notification,
				target_thread_id,
				target_turn_id,
			)? {
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
				return Err(Report::new(failure::turn_failure_from_turn_error(
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

fn notification_targets_turn(notification: &JsonRpcNotification, target_turn_id: &str) -> bool {
	messages::turn_id_from_value(&notification.params)
		.is_none_or(|turn_id| turn_id == target_turn_id)
}
