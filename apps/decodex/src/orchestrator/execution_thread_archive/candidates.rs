use std::collections::HashSet;

#[cfg(test)]
use crate::orchestrator::{AppServerRunResult, IssueRunPlan};
use crate::orchestrator::{
	Result, StateStore, TERMINAL_GUARDED_RUN_STATUS,
	execution_thread_archive::model::{ThreadArchiveCandidate, ThreadArchiveCandidateSource},
};

#[cfg(test)]
pub(in crate::orchestrator) fn completed_issue_thread_archive_candidates(
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	run_result: &AppServerRunResult,
) -> Result<Vec<ThreadArchiveCandidate>> {
	issue_thread_archive_candidates(
		state_store,
		&issue_run.issue.id,
		&issue_run.issue.identifier,
		Some(ThreadArchiveCandidate {
			issue_id: issue_run.issue.id.clone(),
			issue_identifier: issue_run.issue.identifier.clone(),
			run_id: issue_run.run_id.clone(),
			attempt_number: issue_run.attempt_number,
			thread_id: run_result.thread_id.clone(),
			sequence_number: run_result.event_count.saturating_add(1),
		}),
	)
}

pub(in crate::orchestrator) fn issue_thread_archive_candidates(
	state_store: &StateStore,
	issue_id: &str,
	issue_identifier: &str,
	current: Option<ThreadArchiveCandidate>,
) -> Result<Vec<ThreadArchiveCandidate>> {
	let mut seen_thread_ids = HashSet::new();
	let mut candidates = Vec::new();

	if let Some(current) = current {
		push_thread_archive_candidate(
			state_store,
			&mut candidates,
			&mut seen_thread_ids,
			ThreadArchiveCandidateSource {
				run_id: &current.run_id,
				issue_id: &current.issue_id,
				issue_identifier: &current.issue_identifier,
				attempt_number: current.attempt_number,
				thread_id: &current.thread_id,
				sequence_number: Some(current.sequence_number),
			},
		)?;
	}

	for attempt in state_store.list_run_attempts_for_issue(issue_id)? {
		if !completed_issue_archive_attempt_status(attempt.status()) {
			continue;
		}

		if let Some(thread_id) = attempt.thread_id() {
			push_thread_archive_candidate(
				state_store,
				&mut candidates,
				&mut seen_thread_ids,
				ThreadArchiveCandidateSource {
					run_id: attempt.run_id(),
					issue_id: attempt.issue_id(),
					issue_identifier,
					attempt_number: attempt.attempt_number(),
					thread_id,
					sequence_number: None,
				},
			)?;
		}
	}

	Ok(candidates)
}

pub(in crate::orchestrator) fn terminal_thread_archive_backlog_candidates(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Vec<ThreadArchiveCandidate>> {
	let mut seen_thread_ids = HashSet::new();
	let mut candidates = Vec::new();

	for attempt in state_store.list_run_attempts_for_project(project_id)? {
		if !completed_issue_archive_attempt_status(attempt.status()) {
			continue;
		}

		if let Some(thread_id) = attempt.thread_id() {
			push_thread_archive_candidate(
				state_store,
				&mut candidates,
				&mut seen_thread_ids,
				ThreadArchiveCandidateSource {
					run_id: attempt.run_id(),
					issue_id: attempt.issue_id(),
					issue_identifier: attempt.issue_id(),
					attempt_number: attempt.attempt_number(),
					thread_id,
					sequence_number: None,
				},
			)?;
		}
	}

	Ok(candidates)
}

fn completed_issue_archive_attempt_status(status: &str) -> bool {
	matches!(
		status,
		"succeeded" | "failed" | "interrupted" | "terminated" | TERMINAL_GUARDED_RUN_STATUS
	)
}

fn push_thread_archive_candidate(
	state_store: &StateStore,
	candidates: &mut Vec<ThreadArchiveCandidate>,
	seen_thread_ids: &mut HashSet<String>,
	source: ThreadArchiveCandidateSource<'_>,
) -> Result<()> {
	if !seen_thread_ids.insert(source.thread_id.to_owned())
		|| run_has_terminal_thread_archive_event(state_store, source.run_id)?
	{
		return Ok(());
	}

	candidates.push(ThreadArchiveCandidate {
		issue_id: source.issue_id.to_owned(),
		issue_identifier: source.issue_identifier.to_owned(),
		run_id: source.run_id.to_owned(),
		attempt_number: source.attempt_number,
		thread_id: source.thread_id.to_owned(),
		sequence_number: source
			.sequence_number
			.unwrap_or(state_store.event_count(source.run_id)?.saturating_add(1)),
	});

	Ok(())
}

fn run_has_terminal_thread_archive_event(state_store: &StateStore, run_id: &str) -> Result<bool> {
	for event_type in ["thread/archive", "thread/archive/discarded"] {
		if state_store.run_has_protocol_event(run_id, event_type)? {
			return Ok(true);
		}
	}

	Ok(false)
}
