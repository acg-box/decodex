use std::collections::HashSet;

use crate::{
	prelude::{Result, eyre},
	state::{
		project_run_recovery::{
			ProjectRunListingMode, project_lease_run_ids,
			project_run_recovery_candidate_counts_as_project_run, project_run_recovery_candidates,
			project_run_status_from_recovery_candidate,
		},
		runtime_row_parsers::{compare_attempt_records, timestamp_parts},
	},
};

use super::{
	ProjectRunStatus, RunAttempt, RunAttemptRecord, StateStore, compare_project_run_status,
	running_run_attempt_status,
};

impl StateStore {
	/// Insert or update a run attempt record.
	pub fn record_run_attempt(
		&self,
		run_id: &str,
		issue_id: &str,
		attempt_number: i64,
		status: &str,
	) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let project_id = state.project_id_for_run(issue_id, run_id);

		match state.run_attempts.get_mut(run_id) {
			Some(existing) => {
				let retained_project_id =
					(existing.issue_id == issue_id).then(|| existing.project_id.clone()).flatten();

				existing.issue_id = issue_id.to_owned();
				existing.project_id = project_id.or(retained_project_id);
				existing.attempt_number = attempt_number;
				existing.status = status.to_owned();
				existing.updated_at = now.text.clone();
				existing.updated_at_unix = now.unix;
			},
			None => {
				state.run_attempts.insert(
					run_id.to_owned(),
					RunAttemptRecord {
						run_id: run_id.to_owned(),
						project_id,
						issue_id: issue_id.to_owned(),
						attempt_number,
						status: status.to_owned(),
						thread_id: None,
						turn_id: None,
						updated_at: now.text,
						updated_at_unix: now.unix,
					},
				);
			},
		}

		let attempt = state
			.run_attempts
			.get(run_id)
			.ok_or_else(|| eyre::eyre!("Run attempt `{run_id}` was not recorded."))?
			.clone();

		self.upsert_run_attempt_locked(&attempt)
	}

	/// Compute the next attempt number for one issue.
	pub fn next_attempt_number(&self, issue_id: &str) -> Result<i64> {
		let state = self.lock()?;
		let next_attempt = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.map(|attempt| attempt.attempt_number)
			.max()
			.unwrap_or(0)
			+ 1;

		Ok(next_attempt)
	}

	/// Count attempts that consume the retry budget for one issue.
	pub fn retry_budget_attempt_count(&self, issue_id: &str) -> Result<i64> {
		if let Some(sqlite) = self.sqlite.as_ref() {
			let sqlite =
				sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

			return sqlite.retry_budget_attempt_count(issue_id);
		}

		let state = self.lock_without_refresh()?;
		let retry_budget_attempts = state
			.run_attempts
			.values()
			.filter(|attempt| {
				attempt.issue_id == issue_id
					&& matches!(
						attempt.status.as_str(),
						"failed" | "interrupted" | "terminal_guarded"
					)
			})
			.count() as i64;

		Ok(retry_budget_attempts)
	}

	/// Return whether a later attempt for one issue consumed retry budget.
	pub fn issue_has_retry_budget_attempt_after(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<bool> {
		if let Some(sqlite) = self.sqlite.as_ref() {
			let sqlite =
				sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

			return sqlite.issue_has_retry_budget_attempt_after(issue_id, attempt_number);
		}

		let state = self.lock_without_refresh()?;

		Ok(state.run_attempts.values().any(|attempt| {
			attempt.issue_id == issue_id
				&& attempt.attempt_number > attempt_number
				&& matches!(attempt.status.as_str(), "failed" | "interrupted" | "terminal_guarded")
		}))
	}

	/// Attach the active thread identifier to a run attempt.
	pub fn update_run_thread(&self, run_id: &str, thread_id: &str) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;

		if let Some(attempt) = state.run_attempts.get_mut(run_id) {
			attempt.thread_id = Some(thread_id.to_owned());
			attempt.updated_at = now.text;
			attempt.updated_at_unix = now.unix;

			let attempt = attempt.clone();

			return self.upsert_run_attempt_locked(&attempt);
		}

		Ok(())
	}

	/// Attach the active turn identifier to a run attempt.
	pub fn update_run_turn(&self, run_id: &str, turn_id: &str) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;

		if let Some(attempt) = state.run_attempts.get_mut(run_id) {
			attempt.turn_id = Some(turn_id.to_owned());
			attempt.updated_at = now.text;
			attempt.updated_at_unix = now.unix;

			let attempt = attempt.clone();

			return self.upsert_run_attempt_locked(&attempt);
		}

		Ok(())
	}

	/// Update the status for one run attempt.
	pub fn update_run_status(&self, run_id: &str, status: &str) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;

		if let Some(attempt) = state.run_attempts.get_mut(run_id) {
			attempt.status = status.to_owned();
			attempt.updated_at = now.text;
			attempt.updated_at_unix = now.unix;

			let attempt = attempt.clone();

			return self.upsert_run_attempt_locked(&attempt);
		}

		Ok(())
	}

	/// Mark all running run attempts for one issue as succeeded.
	pub fn succeed_running_run_attempts_for_issue(&self, issue_id: &str) -> Result<usize> {
		let now = timestamp_parts();
		let mut state = self.lock()?;
		let mut updated_count = 0;

		for attempt in state
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter(|attempt| running_run_attempt_status(&attempt.status))
		{
			attempt.status = "succeeded".to_owned();
			attempt.updated_at = now.text.clone();
			attempt.updated_at_unix = now.unix;
			updated_count += 1;
		}

		if updated_count > 0 {
			self.persist_runtime_state_locked(&state)?;
		}

		Ok(updated_count)
	}

	/// Read one run attempt.
	pub fn run_attempt(&self, run_id: &str) -> Result<Option<RunAttempt>> {
		let state = self.lock()?;

		Ok(state.run_attempts.get(run_id).map(RunAttemptRecord::as_public))
	}

	/// Read one run attempt by issue and attempt number.
	pub fn run_attempt_for_issue_attempt(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<Option<RunAttempt>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.run_attempt_for_issue_attempt(issue_id, attempt_number)
				.map(|attempt| attempt.map(|attempt| attempt.as_public()));
		}

		let state = self.lock()?;
		let attempt = state
			.run_attempts
			.values()
			.filter(|attempt| {
				attempt.issue_id == issue_id && attempt.attempt_number == attempt_number
			})
			.max_by(|left, right| compare_attempt_records(left, right))
			.map(RunAttemptRecord::as_public);

		Ok(attempt)
	}

	/// Read the latest run attempt for one issue.
	pub fn latest_run_attempt_for_issue(&self, issue_id: &str) -> Result<Option<RunAttempt>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.latest_run_attempt_for_issue(issue_id)
				.map(|attempt| attempt.map(|attempt| attempt.as_public()));
		}

		let state = self.lock()?;
		let attempt = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.max_by(|left, right| compare_attempt_records(left, right))
			.map(RunAttemptRecord::as_public);

		Ok(attempt)
	}

	/// List all locally recorded run attempts for one issue.
	pub fn list_run_attempts_for_issue(&self, issue_id: &str) -> Result<Vec<RunAttempt>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let attempts = sqlite
				.list_run_attempts_for_issue(issue_id)?
				.into_iter()
				.map(|attempt| attempt.as_public())
				.collect();

			return Ok(attempts);
		}

		let state = self.lock()?;
		let mut attempts = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.map(RunAttemptRecord::as_public)
			.collect::<Vec<_>>();

		attempts.sort_by(|left, right| {
			left.attempt_number()
				.cmp(&right.attempt_number())
				.then_with(|| left.run_id().cmp(right.run_id()))
		});

		Ok(attempts)
	}

	/// List all locally recorded run attempts for one registered project.
	pub fn list_run_attempts_for_project(&self, project_id: &str) -> Result<Vec<RunAttempt>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let attempts = sqlite
				.list_run_attempts_for_project(project_id)?
				.into_iter()
				.map(|attempt| attempt.as_public())
				.collect();

			return Ok(attempts);
		}

		let state = self.lock()?;
		let mut attempts = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.project_id.as_deref() == Some(project_id))
			.map(RunAttemptRecord::as_public)
			.collect::<Vec<_>>();

		attempts.sort_by(|left, right| right.run_id().cmp(left.run_id()));

		Ok(attempts)
	}

	/// List recent run attempts for one project, including lease and protocol summary fields.
	pub fn list_recent_runs(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<ProjectRunStatus>> {
		self.list_project_runs(project_id, limit).map(|(_active, recent)| recent)
	}

	/// List active and recent run attempts for one project from one durable snapshot.
	pub(crate) fn list_project_runs(
		&self,
		project_id: &str,
		base_recent_limit: usize,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		self.list_project_runs_with_mode(
			project_id,
			base_recent_limit,
			ProjectRunListingMode::AllowMarkerIdentityPersistence,
		)
	}

	/// List active and recent project runs without persisting marker-derived identities.
	pub(crate) fn list_project_runs_read_only(
		&self,
		project_id: &str,
		base_recent_limit: usize,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		self.list_project_runs_with_mode(
			project_id,
			base_recent_limit,
			ProjectRunListingMode::ReadOnly,
		)
	}

	fn list_project_runs_with_mode(
		&self,
		project_id: &str,
		base_recent_limit: usize,
		mode: ProjectRunListingMode,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		if matches!(mode, ProjectRunListingMode::AllowMarkerIdentityPersistence) {
			self.refresh_run_attempt_identities_from_worktree_markers_locked(
				&mut state, project_id,
			)?;
		}

		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		let lease_run_ids = project_lease_run_ids(&state, project_id, None);

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &lease_run_ids)?;

		let recovery_candidates = project_run_recovery_candidates(&state, project_id, None)?
			.into_iter()
			.filter(|candidate| {
				project_run_recovery_candidate_counts_as_project_run(&state, candidate)
			})
			.collect::<Vec<_>>();
		let recovery_run_ids = recovery_candidates
			.iter()
			.map(|candidate| candidate.run_id().to_owned())
			.collect::<Vec<_>>();

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &recovery_run_ids)?;
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &recovery_run_ids)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		runs.extend(
			recovery_candidates.iter().filter_map(|candidate| {
				project_run_status_from_recovery_candidate(&state, candidate)
			}),
		);
		runs.sort_by(compare_project_run_status);

		let leased_runs =
			runs.iter().filter(|status| status.run_lease()).cloned().collect::<Vec<_>>();
		let recent_limit = base_recent_limit.saturating_add(leased_runs.len());
		let recent_run_ids =
			runs.iter().take(recent_limit).map(|run| run.run_id().to_owned()).collect::<Vec<_>>();
		let mut summary_run_ids =
			leased_runs.iter().map(|run| run.run_id().to_owned()).collect::<Vec<_>>();

		summary_run_ids.extend(recent_run_ids);
		summary_run_ids.sort();
		summary_run_ids.dedup();
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &summary_run_ids)?;
		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &summary_run_ids)?;

		let summary_run_id_set = summary_run_ids.iter().cloned().collect::<HashSet<_>>();
		let mut selected_runs = state
			.run_attempts
			.values()
			.filter(|attempt| summary_run_id_set.contains(&attempt.run_id))
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		selected_runs.extend(
			recovery_candidates
				.iter()
				.filter(|candidate| summary_run_id_set.contains(candidate.run_id()))
				.filter_map(|candidate| {
					project_run_status_from_recovery_candidate(&state, candidate)
				}),
		);
		selected_runs.sort_by(compare_project_run_status);

		let leased_runs =
			selected_runs.iter().filter(|status| status.run_lease()).cloned().collect::<Vec<_>>();
		let mut recent_runs = selected_runs;

		recent_runs.truncate(recent_limit);

		Ok((leased_runs, recent_runs))
	}

	/// List all locally recorded run attempts for one issue in one project.
	pub(crate) fn list_project_issue_runs(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;
		self.refresh_run_attempt_identities_from_worktree_markers_locked(&mut state, project_id)?;
		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		let lease_run_ids = project_lease_run_ids(&state, project_id, Some(issue_id));

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &lease_run_ids)?;

		let recovery_candidates =
			project_run_recovery_candidates(&state, project_id, Some(issue_id))?;
		let recovery_run_ids = recovery_candidates
			.iter()
			.map(|candidate| candidate.run_id().to_owned())
			.collect::<Vec<_>>();
		let run_ids = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.map(|attempt| attempt.run_id.clone())
			.collect::<Vec<_>>();
		let mut summary_run_ids = run_ids;

		summary_run_ids.extend(recovery_run_ids);
		summary_run_ids.sort();
		summary_run_ids.dedup();
		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &summary_run_ids)?;
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &summary_run_ids)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		runs.extend(
			recovery_candidates.iter().filter_map(|candidate| {
				project_run_status_from_recovery_candidate(&state, candidate)
			}),
		);
		runs.sort_by(compare_project_run_status);

		Ok(runs)
	}

	/// List all leased run attempts for one project without applying the recent-run limit.
	pub fn list_leased_runs(&self, project_id: &str) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.run_lease.then_some(status)
			})
			.collect::<Vec<_>>();
		let mut run_ids = runs.iter().map(|run| run.run_id().to_owned()).collect::<Vec<_>>();

		run_ids.sort();
		run_ids.dedup();
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &run_ids)?;

		runs = state
			.run_attempts
			.values()
			.filter(|attempt| run_ids.contains(&attempt.run_id))
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.run_lease.then_some(status)
			})
			.collect::<Vec<_>>();

		runs.sort_by(compare_project_run_status);

		Ok(runs)
	}
}
