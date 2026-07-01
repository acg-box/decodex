#[allow(clippy::wildcard_imports)]
use super::*;

impl SqliteStateStore {
	pub(in crate::state) fn bootstrap_run_control_channels_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS run_control_channels (
	run_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	transport TEXT NOT NULL,
	channel_path TEXT NOT NULL,
	status TEXT NOT NULL,
	published_at TEXT NOT NULL,
	published_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS run_control_channels_project_issue_idx
ON run_control_channels (project_id, issue_id, attempt_number);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_loop_guardrail_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS loop_guardrail_checkpoints (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	reason TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	consecutive_count INTEGER NOT NULL,
	details_json TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, reason)
);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_connector_backoffs_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS connector_backoffs (
	project_id TEXT NOT NULL,
	connector TEXT NOT NULL,
	sync_phase TEXT NOT NULL,
	quota_class TEXT NOT NULL,
	reset_unix_epoch INTEGER NOT NULL,
	reset_source TEXT NOT NULL,
	warning TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, connector)
);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_private_execution_events_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS private_execution_events (
	record_id INTEGER PRIMARY KEY AUTOINCREMENT,
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	event_type TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	recorded_at TEXT NOT NULL,
	recorded_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS private_execution_events_attempt_idx
ON private_execution_events (
	project_id, issue_id, run_id, attempt_number, record_id
);
"#,
		)?;

		Ok(())
	}
}
