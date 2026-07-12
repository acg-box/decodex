#[cfg(not(test))]
use crate::agent;
use crate::{
	agent::{AppServerThreadArchiveOutcome, AppServerThreadArchiveRequest},
	orchestrator::{
		AppServerProcessEnv, AppServerRunResult, IssueRunPlan, ServiceConfig, StateStore,
		WorkflowDocument,
		execution_thread_archive::{candidates, model::ThreadArchiveCandidate},
	},
};

pub(in crate::orchestrator) fn archive_completed_issue_threads_best_effort(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	process_env: &AppServerProcessEnv,
	transport: &str,
	run_result: &AppServerRunResult,
) {
	let current = ThreadArchiveCandidate {
		issue_id: issue_run.issue.id.clone(),
		issue_identifier: issue_run.issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		attempt_number: issue_run.attempt_number,
		thread_id: run_result.thread_id.clone(),
		sequence_number: run_result.event_count.saturating_add(1),
	};

	archive_issue_threads_best_effort(
		project,
		state_store,
		&issue_run.issue.id,
		&issue_run.issue.identifier,
		process_env,
		transport,
		Some(current),
	);
}

pub(in crate::orchestrator) fn reconcile_terminal_thread_archive_backlog_best_effort(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) {
	let process_env = AppServerProcessEnv::default();
	let transport = workflow.frontmatter().agent().transport();
	let candidates = match candidates::terminal_thread_archive_backlog_candidates(
		state_store,
		project.service_id(),
	) {
		Ok(candidates) => candidates,
		Err(error) => {
			tracing::warn!(
				?error,
				project_id = project.service_id(),
				"Failed to list terminal thread archive backlog; skipping this archive reconciliation pass."
			);

			return;
		},
	};

	for candidate in candidates {
		archive_completed_issue_thread_best_effort(
			project,
			state_store,
			&process_env,
			transport,
			&candidate,
		);
	}
}

fn archive_issue_threads_best_effort(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	issue_identifier: &str,
	process_env: &AppServerProcessEnv,
	transport: &str,
	current: Option<ThreadArchiveCandidate>,
) {
	let fallback_candidate = current.clone();
	let candidates = match candidates::issue_thread_archive_candidates(
		state_store,
		project.service_id(),
		issue_id,
		issue_identifier,
		current,
	) {
		Ok(candidates) => candidates,
		Err(error) => {
			tracing::warn!(
				?error,
				project_id = project.service_id(),
				issue_id,
				issue = issue_identifier,
				"Failed to list completed issue threads for archive; archiving current thread only."
			);

			fallback_candidate.into_iter().collect()
		},
	};

	for candidate in candidates {
		archive_completed_issue_thread_best_effort(
			project,
			state_store,
			process_env,
			transport,
			&candidate,
		);
	}
}

fn archive_completed_issue_thread_best_effort(
	project: &ServiceConfig,
	state_store: &StateStore,
	process_env: &AppServerProcessEnv,
	transport: &str,
	candidate: &ThreadArchiveCandidate,
) {
	let archive_request = AppServerThreadArchiveRequest {
		run_id: &candidate.run_id,
		issue_id: &candidate.issue_id,
		attempt_number: candidate.attempt_number,
		listen: transport,
		process_env,
		thread_id: &candidate.thread_id,
		sequence_number: candidate.sequence_number,
	};
	#[cfg(not(test))]
	let archive_result = agent::archive_app_server_thread_after_success(&archive_request, state_store);
	#[cfg(test)]
	let archive_result = {
		state_store
			.append_event(
				archive_request.run_id,
				archive_request.sequence_number,
				"thread/archive",
				&serde_json::json!({
					"threadId": archive_request.thread_id,
					"issueId": archive_request.issue_id,
					"attemptNumber": archive_request.attempt_number,
				})
				.to_string(),
			)
			.map(|()| AppServerThreadArchiveOutcome::Archived)
	};

	match archive_result {
		Ok(AppServerThreadArchiveOutcome::Archived) => tracing::info!(
			project_id = project.service_id(),
			issue_id = candidate.issue_id,
			issue = candidate.issue_identifier,
			run_id = candidate.run_id,
			attempt = candidate.attempt_number,
			thread_id = %candidate.thread_id,
			"Archived completed issue app-server thread."
		),
		Ok(AppServerThreadArchiveOutcome::DiscardedMissingThread) => tracing::info!(
			project_id = project.service_id(),
			issue_id = candidate.issue_id,
			issue = candidate.issue_identifier,
			run_id = candidate.run_id,
			attempt = candidate.attempt_number,
			thread_id = %candidate.thread_id,
			"Discarded completed issue app-server thread archive because the thread is missing."
		),
		Err(error) => tracing::warn!(
			?error,
			project_id = project.service_id(),
			issue_id = candidate.issue_id,
			issue = candidate.issue_identifier,
			run_id = candidate.run_id,
			attempt = candidate.attempt_number,
			thread_id = %candidate.thread_id,
			"Failed to archive completed issue app-server thread; leaving completed run intact."
		),
	}
}
