use color_eyre::Report;

use crate::{
	agent::app_server::{
		protocol::{AppServerClient, ThreadArchiveRequest},
		runtime_types::{AppServerThreadArchiveOutcome, AppServerThreadArchiveRequest},
		session::{self},
	},
	state::StateStore,
};

pub(crate) fn archive_app_server_thread_after_success(
	request: &AppServerThreadArchiveRequest<'_>,
	state_store: &StateStore,
) -> crate::prelude::Result<AppServerThreadArchiveOutcome> {
	let result = match archive_app_server_thread_after_success_inner(request) {
		Ok(()) => Ok(AppServerThreadArchiveOutcome::Archived),
		Err(error) if thread_archive_error_allows_discard(&error) =>
			Ok(AppServerThreadArchiveOutcome::DiscardedMissingThread),
		Err(error) => Err(error),
	};

	record_thread_archive_result_best_effort(state_store, request, result.as_ref());

	result
}

pub(super) fn record_thread_archive_result_best_effort(
	state_store: &StateStore,
	request: &AppServerThreadArchiveRequest<'_>,
	result: std::result::Result<&AppServerThreadArchiveOutcome, &Report>,
) {
	let (event_type, payload) = match result {
		Ok(AppServerThreadArchiveOutcome::Archived) => (
			"thread/archive",
			serde_json::json!({
				"threadId": request.thread_id,
				"issueId": request.issue_id,
				"attemptNumber": request.attempt_number,
			}),
		),
		Ok(AppServerThreadArchiveOutcome::DiscardedMissingThread) => (
			"thread/archive/discarded",
			serde_json::json!({
				"threadId": request.thread_id,
				"issueId": request.issue_id,
				"attemptNumber": request.attempt_number,
				"reason": "missing_thread_or_rollout",
			}),
		),
		Err(error) => (
			"thread/archive/failed",
			serde_json::json!({
				"threadId": request.thread_id,
				"issueId": request.issue_id,
				"attemptNumber": request.attempt_number,
				"error": error.to_string(),
			}),
		),
	};

	if let Err(record_error) = state_store.append_event(
		request.run_id,
		request.sequence_number,
		event_type,
		&payload.to_string(),
	) {
		tracing::warn!(
			?record_error,
			run_id = request.run_id,
			issue_id = request.issue_id,
			attempt = request.attempt_number,
			thread_id = request.thread_id,
			event_type,
			"Failed to record app-server thread archive event."
		);
	}
}

pub(super) fn thread_archive_error_allows_discard(error: &Report) -> bool {
	let message = error.to_string().to_lowercase();

	session::thread_missing_error_message_allows_discard(&message)
		|| message.contains("already archived")
}

fn archive_app_server_thread_after_success_inner(
	request: &AppServerThreadArchiveRequest<'_>,
) -> crate::prelude::Result<()> {
	let expected_codex_home = request.process_env.resolve_codex_home_env()?;
	let mut client = AppServerClient::spawn(request.listen, request.process_env)?;
	let initialize_response = client.initialize(false)?;

	session::validate_initialize_codex_home(&expected_codex_home, &initialize_response)?;

	client.mark_initialized()?;
	client.archive_thread(ThreadArchiveRequest { thread_id: request.thread_id.to_owned() })?;

	Ok(())
}
