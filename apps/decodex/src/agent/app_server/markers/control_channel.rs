use std::{
	fs,
	path::{Path, PathBuf},
};

use crate::{
	agent::app_server::{
		AppServerRunRequest, RUN_CONTROL_CHANNEL_DIR, RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
		RunControlChannel, StateStore,
	},
	prelude::Result,
	state,
};

pub(in crate::agent::app_server) fn publish_run_control_channel_for_request(
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
) -> Result<Option<RunControlChannel>> {
	let Some(marker_path) = request.activity_marker_path.as_ref() else {
		return Ok(None);
	};
	let channel_path =
		run_control_channel_path(marker_path, &request.run_id, request.attempt_number);

	write_run_control_channel_file(&channel_path, request)?;

	let channel = state_store.publish_run_control_channel_for_active_attempt(
		&request.run_id,
		request.attempt_number,
		&channel_path,
		RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
	)?;

	if let Some(channel) = channel.as_ref() {
		state_store.append_private_execution_event(
			channel.project_id(),
			channel.issue_id(),
			channel.run_id(),
			channel.attempt_number(),
			"control_channel_published",
			serde_json::json!({
				"schema": "decodex.run_control_channel/v1",
				"transport": channel.transport(),
				"channel_path": channel.channel_path().display().to_string(),
				"status": channel.status(),
				"published_at": channel.published_at(),
			}),
		)?;
	}

	Ok(channel)
}

fn run_control_channel_path(marker_path: &Path, run_id: &str, attempt_number: i64) -> PathBuf {
	marker_path
		.join(RUN_CONTROL_CHANNEL_DIR)
		.join(format!("{}-{attempt_number}.channel", sanitize_run_control_path_segment(run_id)))
}

fn sanitize_run_control_path_segment(value: &str) -> String {
	let sanitized = value
		.chars()
		.map(|character| {
			if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
				character
			} else {
				'_'
			}
		})
		.collect::<String>();

	if sanitized.is_empty() { String::from("run") } else { sanitized }
}

fn write_run_control_channel_file(
	channel_path: &Path,
	request: &AppServerRunRequest<'_>,
) -> Result<()> {
	if let Some(parent) = channel_path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::write(
		channel_path,
		format!(
			"schema=decodex.run_control_channel/v1\nrun_id={}\nissue_id={}\nattempt_number={}\ntransport={}\n",
			request.run_id,
			request.issue_id,
			request.attempt_number,
			state::RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
		),
	)?;

	Ok(())
}
