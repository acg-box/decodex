/// Active lease for one issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueLease {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) issue_state: String,
}
impl IssueLease {
	/// Local project identifier owning this lease.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier owning the lease.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Run identifier holding the lease.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Tracker state representing the dispatched run.
	#[cfg(test)]
	pub fn issue_state(&self) -> &str {
		&self.issue_state
	}
}
