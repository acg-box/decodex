/// Persistent run attempt metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunAttempt {
	pub(in crate::state) run_id: String,
	pub(in crate::state) project_id: Option<String>,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) status: String,
	pub(in crate::state) thread_id: Option<String>,
	pub(in crate::state) turn_id: Option<String>,
}
impl RunAttempt {
	/// Stable run identifier.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Canonical project lane for the run, absent only on unmigrated legacy evidence.
	pub fn project_id(&self) -> Option<&str> {
		self.project_id.as_deref()
	}

	/// Issue identifier for the run.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Attempt number for this run.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Current local status for the run.
	pub fn status(&self) -> &str {
		&self.status
	}

	/// Thread identifier returned by `app-server`, when known.
	pub fn thread_id(&self) -> Option<&str> {
		self.thread_id.as_deref()
	}

	/// Latest turn identifier returned by `app-server`, when known.
	pub fn turn_id(&self) -> Option<&str> {
		self.turn_id.as_deref()
	}
}
