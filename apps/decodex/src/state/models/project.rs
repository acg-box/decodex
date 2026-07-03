use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{
	config::ServiceConfig,
	state,
	state::models::{ChildAgentActivitySummary, ProtocolActivitySummary, RunControlChannel},
};

pub(crate) const WORKTREE_PROVENANCE_FILESYSTEM_SCAN: &str = "filesystem_scan";
pub(crate) const WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN: &str = "git_hygiene_scan";
pub(crate) const WORKTREE_PROVENANCE_LEGACY_UNKNOWN: &str = "legacy_unknown";
pub(crate) const WORKTREE_PROVENANCE_RUNTIME_RECOVERED: &str = "runtime_recovered";
pub(crate) const WORKTREE_PROVENANCE_RUNTIME_RECORDED: &str = "runtime_recorded";

/// One private, local-only execution event retained in the runtime SQLite ledger.
#[derive(Clone, Debug, PartialEq)]
pub struct PrivateExecutionEvent {
	pub(in crate::state) record_id: i64,
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) event_type: String,
	pub(in crate::state) payload: Value,
	pub(in crate::state) recorded_at: String,
	pub(in crate::state) recorded_at_unix: i64,
}
impl PrivateExecutionEvent {
	/// Monotonic local row id assigned by the runtime store.
	pub fn record_id(&self) -> i64 {
		self.record_id
	}

	/// Local project identifier owning the evidence row.
	#[cfg(test)]
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier for this private evidence row.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Run identifier for this private evidence row.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Attempt number for this private evidence row.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Private event type chosen by the runtime or issue-scoped tool path.
	pub fn event_type(&self) -> &str {
		&self.event_type
	}

	/// Structured JSON payload kept local to the runtime store.
	pub fn payload(&self) -> &Value {
		&self.payload
	}

	/// UTC timestamp when the runtime store recorded this row.
	pub fn recorded_at(&self) -> &str {
		&self.recorded_at
	}

	/// Unix timestamp when the runtime store recorded this row.
	pub fn recorded_at_unix(&self) -> i64 {
		self.recorded_at_unix
	}
}

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

/// Worktree mapping for one issue lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeMapping {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) worktree_path: PathBuf,
	pub(in crate::state) provenance: WorktreeProvenance,
}
impl WorktreeMapping {
	/// Local project identifier owning this lane.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier for this lane.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Branch name used for the lane.
	pub fn branch_name(&self) -> &str {
		&self.branch_name
	}

	/// Filesystem path to the worktree checkout.
	pub fn worktree_path(&self) -> &Path {
		&self.worktree_path
	}

	/// Durable provenance captured when Decodex recorded or migrated this mapping.
	pub fn provenance(&self) -> &WorktreeProvenance {
		&self.provenance
	}
}

/// Durable provenance for a retained worktree mapping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeProvenance {
	pub(in crate::state) source: String,
	pub(in crate::state) created_at_unix: Option<i64>,
	pub(in crate::state) updated_at_unix: Option<i64>,
}
impl WorktreeProvenance {
	/// Source that created or last classified this mapping.
	pub fn source(&self) -> &str {
		&self.source
	}

	/// Unix timestamp for when this mapping was first recorded, when available.
	pub fn created_at_unix(&self) -> Option<i64> {
		self.created_at_unix
	}

	/// Unix timestamp for when this mapping was last refreshed, when available.
	pub fn updated_at_unix(&self) -> Option<i64> {
		self.updated_at_unix
	}

	/// Whether this mapping was migrated from a legacy row without durable provenance.
	pub fn is_legacy_unknown(&self) -> bool {
		self.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN
	}
}

/// Project-scoped external connector backoff retained in the runtime store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectorBackoff {
	pub(in crate::state) project_id: String,
	pub(in crate::state) connector: String,
	pub(in crate::state) sync_phase: String,
	pub(in crate::state) quota_class: String,
	pub(in crate::state) reset_unix_epoch: i64,
	pub(in crate::state) reset_source: String,
	pub(in crate::state) warning: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ConnectorBackoff {
	/// Local project identifier affected by this connector backoff.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Connector name, such as `linear`.
	pub fn connector(&self) -> &str {
		&self.connector
	}

	/// Runtime phase that last observed the connector backoff.
	pub fn sync_phase(&self) -> &str {
		&self.sync_phase
	}

	/// Quota class backing the pause.
	pub fn quota_class(&self) -> &str {
		&self.quota_class
	}

	/// Unix epoch when Decodex may retry the connector.
	pub fn reset_unix_epoch(&self) -> i64 {
		self.reset_unix_epoch
	}

	/// Source for the reset time.
	pub fn reset_source(&self) -> &str {
		&self.reset_source
	}

	/// Snapshot warning represented by this backoff.
	pub fn warning(&self) -> &str {
		&self.warning
	}

	/// Timestamp when Decodex stored the backoff.
	pub fn updated_at(&self) -> &str {
		&self.updated_at
	}

	/// Unix timestamp when Decodex stored the backoff.
	pub fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// Registered repo target managed by the local Decodex control plane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectRegistration {
	pub(in crate::state) service_id: String,
	pub(in crate::state) config_path: PathBuf,
	pub(in crate::state) repo_root: PathBuf,
	pub(in crate::state) worktree_root: PathBuf,
	pub(in crate::state) workflow_path: PathBuf,
	pub(in crate::state) tracker_api_key_env_var: String,
	pub(in crate::state) github_token_env_var: String,
	pub(in crate::state) enabled: bool,
	pub(in crate::state) config_fingerprint: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl ProjectRegistration {
	/// Build a registry row from a Decodex project config.
	pub(crate) fn from_config(
		service_id: &str,
		config_path: &Path,
		config: &ServiceConfig,
		enabled: bool,
		config_fingerprint: &str,
	) -> Self {
		let now = state::timestamp_parts();

		Self {
			service_id: service_id.to_owned(),
			config_path: config_path.to_path_buf(),
			repo_root: config.repo_root().to_path_buf(),
			worktree_root: config.worktree_root().to_path_buf(),
			workflow_path: config.workflow_path().to_path_buf(),
			tracker_api_key_env_var: config.tracker().api_key_env_var().to_owned(),
			github_token_env_var: config.github().token_env_var().to_owned(),
			enabled,
			config_fingerprint: config_fingerprint.to_owned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		}
	}

	/// Stable service id from the project config.
	pub(crate) fn service_id(&self) -> &str {
		&self.service_id
	}

	/// Absolute config path registered for this project.
	pub(crate) fn config_path(&self) -> &Path {
		&self.config_path
	}

	/// Absolute repository root for this project.
	pub(crate) fn repo_root(&self) -> &Path {
		&self.repo_root
	}

	/// Absolute worktree root for this project.
	pub(crate) fn worktree_root(&self) -> &Path {
		&self.worktree_root
	}

	/// Absolute workflow path registered for this project.
	pub(crate) fn workflow_path(&self) -> &Path {
		&self.workflow_path
	}

	/// Environment variable name for the tracker API key.
	pub(crate) fn tracker_api_key_env_var(&self) -> &str {
		&self.tracker_api_key_env_var
	}

	/// Environment variable name for the GitHub token.
	pub(crate) fn github_token_env_var(&self) -> &str {
		&self.github_token_env_var
	}

	/// Whether the project participates in `decodex serve`.
	pub(crate) fn enabled(&self) -> bool {
		self.enabled
	}

	/// Last config fingerprint registered for this project.
	pub(crate) fn config_fingerprint(&self) -> &str {
		&self.config_fingerprint
	}

	/// Last registry update timestamp.
	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	/// Last registry update timestamp as Unix epoch seconds.
	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}

	/// Set whether the registered project is enabled.
	pub(in crate::state) fn set_enabled(&mut self, enabled: bool) {
		self.enabled = enabled;

		let now = state::timestamp_parts();

		self.updated_at = now.text;
		self.updated_at_unix = now.unix;
	}
}

pub(crate) fn worktree_provenance(
	source: impl Into<String>,
	created_at_unix: Option<i64>,
	updated_at_unix: Option<i64>,
) -> WorktreeProvenance {
	WorktreeProvenance { source: source.into(), created_at_unix, updated_at_unix }
}
