use crate::state::sqlite_store::mutations::{
	self, ConnectorBackoff, ProjectRegistration, Result, SqliteStateStore,
	connector_backoff_from_row,
};

impl SqliteStateStore {
	pub(in crate::state) fn delete_project(&mut self, service_id: &str) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"DELETE FROM projects WHERE service_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM connector_backoffs WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM run_control_channels WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM decision_contracts WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM autonomy_objectives WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM autonomy_signals WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM autonomy_proposals WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM execution_programs WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM program_intake_plans WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM evidence_artifacts WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE project_id = ?1",
			mutations::params![service_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	pub(in crate::state) fn upsert_project(&self, project: &ProjectRegistration) -> Result<()> {
		self.connection.execute(
			"INSERT OR REPLACE INTO projects (
					service_id, config_path, repo_root, worktree_root, workflow_path,
					tracker_api_key_env_var, github_token_env_var, enabled, config_fingerprint,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			mutations::params![
				project.service_id(),
				project.config_path().to_string_lossy().as_ref(),
				project.repo_root().to_string_lossy().as_ref(),
				project.worktree_root().to_string_lossy().as_ref(),
				project.workflow_path().to_string_lossy().as_ref(),
				project.tracker_api_key_env_var(),
				project.github_token_env_var(),
				if project.enabled() { 1_i64 } else { 0_i64 },
				project.config_fingerprint(),
				project.updated_at(),
				project.updated_at_unix(),
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn delete_connector_backoff(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM connector_backoffs WHERE project_id = ?1 AND connector = ?2",
			mutations::params![project_id, connector],
		)?;

		Ok(())
	}

	pub(in crate::state) fn connector_backoff(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<Option<ConnectorBackoff>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, connector, sync_phase, quota_class, reset_unix_epoch,
			 reset_source, warning, updated_at, updated_at_unix
			 FROM connector_backoffs
			 WHERE project_id = ?1 AND connector = ?2
			 LIMIT 1",
		)?;
		let mut rows = statement.query(mutations::params![project_id, connector])?;

		Ok(rows.next()?.map(connector_backoff_from_row).transpose()?)
	}
}
