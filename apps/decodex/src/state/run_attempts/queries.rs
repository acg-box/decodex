use crate::{
	prelude::{Result, eyre},
	state::{RunAttempt, RunAttemptRecord, StateStore, runtime_row_parsers},
};

impl StateStore {
	/// Read one run attempt.
	pub fn run_attempt(&self, run_id: &str) -> Result<Option<RunAttempt>> {
		let state = self.lock()?;

		Ok(state.run_attempts.get(run_id).map(RunAttemptRecord::as_public))
	}

	/// Read one run attempt by issue and attempt number.
	#[cfg(test)]
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
			.max_by(|left, right| runtime_row_parsers::compare_attempt_records(left, right))
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
			.max_by(|left, right| runtime_row_parsers::compare_attempt_records(left, right))
			.map(RunAttemptRecord::as_public);

		Ok(attempt)
	}

	/// List all locally recorded run attempts for one issue.
	#[cfg(test)]
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

	/// List all locally recorded run attempts for one canonical lane.
	pub fn list_run_attempts_for_lane(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Vec<RunAttempt>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			return Ok(sqlite
				.list_run_attempts_for_lane(project_id, issue_id)?
				.into_iter()
				.map(|attempt| attempt.as_public())
				.collect());
		}

		let state = self.lock()?;
		let mut attempts = state
			.run_attempts
			.values()
			.filter(|attempt| {
				attempt.project_id.as_deref() == Some(project_id) && attempt.issue_id == issue_id
			})
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
}
