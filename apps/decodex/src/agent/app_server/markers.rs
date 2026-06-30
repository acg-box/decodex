use std::{
	fs,
	path::{Path, PathBuf},
};

use super::{
	AppServerRunRequest, CodexAccountActivitySummary, CodexAccountMarker, EffectiveRuntimeMarker,
	EffectiveThreadConfig, RUN_CONTROL_CHANNEL_DIR, RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
	RUN_OPERATION_AGENT_RUN, RUN_OPERATION_APP_SERVER_PREFLIGHT, RunControlChannel, StateStore,
	state,
};

pub(super) fn publish_run_control_channel_for_request(
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
) -> crate::prelude::Result<Option<RunControlChannel>> {
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
) -> crate::prelude::Result<()> {
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

pub(super) fn write_activity_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
) {
	if let Err(error) = state::write_run_operation_marker(
		marker_path,
		run_id,
		attempt_number,
		RUN_OPERATION_AGENT_RUN,
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree activity marker."
		);
	}
}

pub(super) fn write_activity_marker_best_effort_for_request(request: &AppServerRunRequest<'_>) {
	if let Some(marker_path) = request.activity_marker_path.as_ref() {
		write_activity_marker_best_effort(marker_path, &request.run_id, request.attempt_number);
	}
}

pub(super) fn write_capability_preflight_marker_best_effort(request: &AppServerRunRequest<'_>) {
	if let Some(marker_path) = request.activity_marker_path.as_ref()
		&& let Err(error) = state::write_run_operation_marker(
			marker_path,
			&request.run_id,
			request.attempt_number,
			RUN_OPERATION_APP_SERVER_PREFLIGHT,
		) {
		tracing::warn!(
			?error,
			run_id = request.run_id,
			attempt_number = request.attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree app-server preflight marker."
		);
	}
}

pub(super) fn write_protocol_activity_marker_best_effort(
	marker_path: &Path,
	activity: &state::ProtocolActivityMarker<'_>,
) {
	if let Err(error) = state::write_run_protocol_activity_marker(marker_path, activity) {
		tracing::warn!(
			?error,
			run_id = activity.run_id,
			attempt_number = activity.attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree protocol-activity marker."
		);
	}
}

pub(super) fn write_turn_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	turn_id: &str,
) {
	if let Err(error) = state::write_run_turn_marker(marker_path, run_id, attempt_number, turn_id) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree turn marker."
		);
	}
}

pub(super) fn write_thread_status_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: Option<&str>,
	turn_id: Option<&str>,
	thread_status: &str,
	thread_active_flags: &[String],
) {
	if let Err(error) = state::write_run_thread_status_marker(
		marker_path,
		run_id,
		attempt_number,
		thread_id,
		turn_id,
		thread_status,
		thread_active_flags,
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree thread-status marker."
		);
	}
}

pub(super) fn write_effective_runtime_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: Option<&str>,
	turn_id: Option<&str>,
	runtime: &EffectiveThreadConfig,
) {
	if let Err(error) = state::write_run_effective_runtime_marker(
		marker_path,
		run_id,
		attempt_number,
		&EffectiveRuntimeMarker {
			thread_id,
			turn_id,
			effective_model: &runtime.model,
			effective_model_provider: &runtime.model_provider,
			effective_cwd: &runtime.cwd,
			effective_approval_policy: &runtime.approval_policy,
			effective_approvals_reviewer: &runtime.approvals_reviewer,
			effective_sandbox_mode: &runtime.sandbox_mode,
		},
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree effective-runtime marker."
		);
	}
}

pub(super) fn write_codex_account_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	summary: &CodexAccountActivitySummary,
	account_summaries: &[CodexAccountActivitySummary],
) {
	if let Err(error) = state::write_run_account_marker(
		marker_path,
		&CodexAccountMarker {
			run_id,
			attempt_number,
			account: summary,
			accounts: account_summaries,
		},
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree Codex account marker."
		);
	}
}

pub(super) fn write_thread_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: &str,
) {
	if let Err(error) =
		state::write_run_thread_marker(marker_path, run_id, attempt_number, thread_id)
	{
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree thread marker."
		);
	}
}
