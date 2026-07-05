use crate::{
	agent::{
		app_server::{runtime_types::RunRecorder, turn_loop::messages},
		json_rpc::JsonRpcNotification,
	},
	prelude::Result,
};

pub(in crate::agent::app_server::turn_loop) fn adopt_thread_bound_notification_turn_id(
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
