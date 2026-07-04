use std::path::{Path, PathBuf};

/// Local control capability published by one running run attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunControlChannel {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) transport: String,
	pub(in crate::state) channel_path: PathBuf,
	pub(in crate::state) status: String,
	pub(in crate::state) published_at: String,
	pub(in crate::state) published_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl RunControlChannel {
	/// Local project identifier owning this control channel.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier owning this control channel.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Stable run identifier owning this control channel.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Attempt number owning this control channel.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Local transport mechanism for this control channel.
	pub fn transport(&self) -> &str {
		&self.transport
	}

	/// Local path used by this control channel.
	pub fn channel_path(&self) -> &Path {
		&self.channel_path
	}

	/// Runtime status for this control channel.
	pub fn status(&self) -> &str {
		&self.status
	}

	/// UTC timestamp when this control channel was first published.
	pub fn published_at(&self) -> &str {
		&self.published_at
	}

	/// UTC timestamp when this control channel was last updated.
	pub fn updated_at(&self) -> &str {
		&self.updated_at
	}
}
