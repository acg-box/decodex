use crate::{
	prelude::{Result, eyre},
	state::{self, RunAttemptRecord, StateStore, runtime_row_parsers},
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
		let now = runtime_row_parsers::timestamp_parts();
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
		let now = runtime_row_parsers::timestamp_parts();
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
		let now = runtime_row_parsers::timestamp_parts();
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
		let now = runtime_row_parsers::timestamp_parts();
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
		let now = runtime_row_parsers::timestamp_parts();
		let mut state = self.lock()?;
		let mut updated_count = 0;

		for attempt in state
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter(|attempt| state::running_run_attempt_status(&attempt.status))
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
}
