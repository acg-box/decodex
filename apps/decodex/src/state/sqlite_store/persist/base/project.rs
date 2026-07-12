use crate::state::sqlite_store::persist::{self, Result, StateData, Transaction};

pub(in crate::state::sqlite_store) fn persist_projects(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for project in state.projects.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO projects (
					service_id, config_path, repo_root, worktree_root, workflow_path,
					tracker_api_key_env_var, github_token_env_var, github_owner,
					github_repository, tracker_team_id, routing_label, enabled,
					config_fingerprint, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			persist::params![
				project.service_id(),
				project.config_path().to_string_lossy().as_ref(),
				project.repo_root().to_string_lossy().as_ref(),
				project.worktree_root().to_string_lossy().as_ref(),
				project.workflow_path().to_string_lossy().as_ref(),
				project.tracker_api_key_env_var(),
				project.github_token_env_var(),
				project.binding().github_owner(),
				project.binding().github_repository(),
				project.binding().tracker_team_id(),
				project.binding().routing_label(),
				if project.enabled() { 1_i64 } else { 0_i64 },
				project.config_fingerprint(),
				project.updated_at(),
				project.updated_at_unix(),
			],
		)?;
	}

	Ok(())
}

#[cfg(test)]
pub(in crate::state::sqlite_store) fn update_run_attempt_project(
	transaction: &Transaction<'_>,
	project_id: &str,
	issue_id: &str,
	run_id: Option<&str>,
) -> Result<()> {
	match run_id {
		Some(run_id) => {
			transaction.execute(
				"UPDATE run_attempts SET project_id = ?1 WHERE issue_id = ?2 AND run_id = ?3",
				persist::params![project_id, issue_id, run_id],
			)?;
		},
		None => {
			transaction.execute(
				"UPDATE run_attempts SET project_id = ?1 WHERE issue_id = ?2",
				persist::params![project_id, issue_id],
			)?;
		},
	}

	Ok(())
}

#[cfg(test)]
pub(in crate::state::sqlite_store) fn persist_leases(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for lease in state.leases.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO leases (issue_id, project_id, run_id, issue_state) \
				 VALUES (?1, ?2, ?3, ?4)",
			persist::params![
				lease.issue_id(),
				lease.project_id(),
				lease.run_id(),
				lease.issue_state()
			],
		)?;
	}

	Ok(())
}

#[cfg(test)]
pub(in crate::state::sqlite_store) fn persist_worktrees(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for mapping in state.worktrees.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO worktrees (
				issue_id, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
			persist::params![
				&mapping.issue_id,
				&mapping.project_id,
				&mapping.branch_name,
				mapping.worktree_path.to_string_lossy().as_ref(),
				&mapping.provenance_source,
				mapping.created_at_unix,
				mapping.updated_at_unix,
			],
		)?;
	}

	Ok(())
}
