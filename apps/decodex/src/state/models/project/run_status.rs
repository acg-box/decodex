use std::path::{Path, PathBuf};

use crate::state::{ChildAgentActivitySummary, ProtocolActivitySummary, RunControlChannel};

/// Project-scoped operator view of one run attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRunStatus {
	pub(in crate::state) run_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) status: String,
	pub(in crate::state) thread_id: Option<String>,
	pub(in crate::state) turn_id: Option<String>,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
	pub(in crate::state) branch_name: Option<String>,
	pub(in crate::state) worktree_path: Option<PathBuf>,
	pub(in crate::state) run_lease: bool,
	pub(in crate::state) event_count: i64,
	pub(in crate::state) last_event_type: Option<String>,
	pub(in crate::state) last_event_at: Option<String>,
	pub(in crate::state) last_event_at_unix: Option<i64>,
	pub(in crate::state) control_channel: Option<RunControlChannel>,
	pub(in crate::state) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(in crate::state) protocol_activity: Option<ProtocolActivitySummary>,
	pub(in crate::state) recovery_source: String,
	pub(in crate::state) recovery_evidence: Vec<String>,
	pub(in crate::state) recovery_gaps: Vec<String>,
}
impl ProjectRunStatus {
	/// Stable run identifier.
	pub fn run_id(&self) -> &str {
		&self.run_id
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

	/// Timestamp of the latest run-attempt status update.
	pub fn updated_at(&self) -> &str {
		&self.updated_at
	}

	/// Branch name for the retained lane, when known.
	pub fn branch_name(&self) -> Option<&str> {
		self.branch_name.as_deref()
	}

	/// Filesystem path to the retained worktree, when known.
	pub fn worktree_path(&self) -> Option<&Path> {
		self.worktree_path.as_deref()
	}

	/// Whether this run still holds the active local lease.
	pub fn run_lease(&self) -> bool {
		self.run_lease
	}

	/// Number of recorded protocol events for the run.
	pub fn event_count(&self) -> i64 {
		self.event_count
	}

	/// Latest recorded protocol event type, when one exists.
	pub fn last_event_type(&self) -> Option<&str> {
		self.last_event_type.as_deref()
	}

	/// Timestamp of the latest recorded protocol event, when one exists.
	pub fn last_event_at(&self) -> Option<&str> {
		self.last_event_at.as_deref()
	}

	/// Local control capability published by this run attempt, when one exists.
	pub fn control_channel(&self) -> Option<&RunControlChannel> {
		self.control_channel.as_ref()
	}

	pub(crate) fn child_agent_activity(&self) -> Option<&ChildAgentActivitySummary> {
		self.child_agent_activity.as_ref()
	}

	pub(crate) fn protocol_activity(&self) -> Option<&ProtocolActivitySummary> {
		self.protocol_activity.as_ref()
	}

	pub(crate) fn recovery_source(&self) -> &str {
		&self.recovery_source
	}

	pub(crate) fn recovery_evidence(&self) -> &[String] {
		&self.recovery_evidence
	}

	pub(crate) fn recovery_gaps(&self) -> &[String] {
		&self.recovery_gaps
	}

	/// Unix timestamp of the latest recorded protocol event, when one exists.
	pub(crate) fn last_event_at_unix(&self) -> Option<i64> {
		self.last_event_at_unix
	}

	pub(crate) fn last_run_activity_unix_epoch(&self) -> i64 {
		match self.last_event_at_unix {
			Some(last_event_at_unix) => self.updated_at_unix.max(last_event_at_unix),
			None => self.updated_at_unix,
		}
	}
}
