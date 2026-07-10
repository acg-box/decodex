use crate::state::sqlite_store::schema::{Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn bootstrap_decision_contracts_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS decision_contracts (
	project_id TEXT NOT NULL,
	contract_id TEXT NOT NULL,
	source_issue_id TEXT,
	status TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, contract_id)
);
CREATE INDEX IF NOT EXISTS decision_contracts_source_issue_idx
ON decision_contracts (project_id, source_issue_id, updated_at_unix);
CREATE INDEX IF NOT EXISTS decision_contracts_status_idx
ON decision_contracts (project_id, status, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_autonomy_objectives_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_objectives (
	project_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	version INTEGER NOT NULL,
	state TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, objective_id, version)
);
CREATE INDEX IF NOT EXISTS autonomy_objectives_project_state_idx
ON autonomy_objectives (project_id, state, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_objectives_history_idx
ON autonomy_objectives (project_id, objective_id, version);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_autonomy_runtime_policies_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_runtime_policies (
	project_id TEXT NOT NULL,
	policy_id TEXT NOT NULL,
	policy_version TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	objective_version INTEGER NOT NULL,
	objective_digest TEXT NOT NULL,
	authority_ref TEXT NOT NULL,
	accepted_by TEXT NOT NULL,
	accepted_at TEXT NOT NULL,
	acceptance_source TEXT NOT NULL,
	public_non_goals_json TEXT NOT NULL,
	PRIMARY KEY (project_id, policy_id, policy_version)
);
CREATE INDEX IF NOT EXISTS autonomy_runtime_policies_objective_idx
ON autonomy_runtime_policies (project_id, objective_id, objective_version);
CREATE TABLE IF NOT EXISTS autonomy_runtime_policy_receipts (
	project_id TEXT NOT NULL,
	receipt_id TEXT NOT NULL,
	principal TEXT NOT NULL,
	candidate_digest TEXT NOT NULL,
	candidate_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	expires_at_unix INTEGER NOT NULL,
	consumed_at TEXT,
	PRIMARY KEY (project_id, receipt_id)
);
CREATE INDEX IF NOT EXISTS autonomy_runtime_policy_receipts_expiry_idx
ON autonomy_runtime_policy_receipts (project_id, expires_at_unix, consumed_at);
"#,
		)?;
		self.ensure_column(
			"autonomy_runtime_policies",
			"objective_digest",
			"ALTER TABLE autonomy_runtime_policies ADD COLUMN objective_digest TEXT NOT NULL DEFAULT ''",
		)?;
		self.connection
			.execute("DELETE FROM autonomy_runtime_policies WHERE objective_digest = ''", [])?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_autonomy_signals_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_signals (
	project_id TEXT NOT NULL,
	signal_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	objective_version INTEGER NOT NULL,
	kind TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	freshness TEXT NOT NULL,
	evidence_class TEXT NOT NULL,
	confidence TEXT NOT NULL,
	privacy TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, signal_id)
);
CREATE INDEX IF NOT EXISTS autonomy_signals_objective_idx
ON autonomy_signals (project_id, objective_id, objective_version, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_signals_recent_idx
ON autonomy_signals (project_id, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	pub(in crate::state) fn bootstrap_autonomy_proposals_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_proposals (
	project_id TEXT NOT NULL,
	proposal_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	objective_version INTEGER NOT NULL,
	state TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	source_family TEXT NOT NULL,
	intended_surface TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, proposal_id)
);
CREATE INDEX IF NOT EXISTS autonomy_proposals_objective_idx
ON autonomy_proposals (project_id, objective_id, objective_version, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_proposals_state_idx
ON autonomy_proposals (project_id, state, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_proposals_recent_idx
ON autonomy_proposals (project_id, updated_at_unix);
"#,
		)?;

		Ok(())
	}
}
