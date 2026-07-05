use crate::{
	agent::app_server::{AppServerClient, RunRecorder, server_requests, turn_loop::messages},
	prelude::Result,
};

pub(in crate::agent::app_server) fn flush_pending_messages(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	target_thread_id: Option<&str>,
) -> Result<()> {
	for message in client.drain_pending() {
		if messages::targets_thread(&message, target_thread_id) {
			recorder.record(messages::message_type(&message), &message.raw)?;

			server_requests::apply_protocol_message_side_effects(recorder, &message)?;
		}
	}

	Ok(())
}
