use color_eyre::eyre::Report;

use crate::{
	agent::{app_server::turn_failure::AppServerTurnFailure, json_rpc::AppServerOutputTimeout},
	prelude::eyre,
};

pub(in crate::agent::app_server) fn is_app_server_output_timeout(error: &Report) -> bool {
	error.downcast_ref::<AppServerOutputTimeout>().is_some()
}

pub(in crate::agent::app_server::turn_loop::wait) fn turn_wait_timeout_error(
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
