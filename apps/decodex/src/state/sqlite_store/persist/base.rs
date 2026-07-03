use crate::state::sqlite_store::persist::{
	self, ChildAgentActivitySummary, Result, StateData, Transaction,
};

pub(in crate::state::sqlite_store) fn persist_projects(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for project in state.projects.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO projects (
					service_id, config_path, repo_root, worktree_root, workflow_path,
					tracker_api_key_env_var, github_token_env_var, enabled, config_fingerprint,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			persist::params![
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
	}

	Ok(())
}

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

pub(in crate::state::sqlite_store) fn persist_run_attempts(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for attempt in state.run_attempts.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO run_attempts (
					run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			persist::params![
				&attempt.run_id,
				attempt.project_id.as_deref(),
				&attempt.issue_id,
				attempt.attempt_number,
				&attempt.status,
				attempt.thread_id.as_deref(),
				attempt.turn_id.as_deref(),
				&attempt.updated_at,
				attempt.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_run_control_channels(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for channel in state.control_channels.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO run_control_channels (
					run_id, project_id, issue_id, attempt_number, transport, channel_path, status,
					published_at, published_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			persist::params![
				&channel.run_id,
				&channel.project_id,
				&channel.issue_id,
				channel.attempt_number,
				&channel.transport,
				channel.channel_path.to_string_lossy().as_ref(),
				&channel.status,
				&channel.published_at,
				channel.published_at_unix,
				&channel.updated_at,
				channel.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_protocol_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for (run_id, events) in &state.events {
		for event in events {
			transaction.execute(
				"INSERT OR REPLACE INTO protocol_events (
						run_id, sequence_number, event_type, payload_sha256, created_at,
						created_at_unix
					) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
				persist::params![
					run_id,
					event.sequence_number,
					&event.event_type,
					&event.payload_sha256,
					&event.created_at,
					event.created_at_unix,
				],
			)?;
		}
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_run_activity_summaries(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for summary in state.run_activity_summaries.values() {
		let child_agent_activity_json = summary
			.child_agent_activity
			.as_ref()
			.cloned()
			.map(ChildAgentActivitySummary::sealed_durable)
			.map(|summary| serde_json::to_string(&summary))
			.transpose()?;
		let protocol_activity_json =
			summary.protocol_activity.as_ref().map(serde_json::to_string).transpose()?;

		transaction.execute(
			"INSERT OR REPLACE INTO run_activity_summaries (
					run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			persist::params![
				&summary.run_id,
				summary.attempt_number,
				child_agent_activity_json.as_deref(),
				protocol_activity_json.as_deref(),
				&summary.updated_at,
				summary.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

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

pub(in crate::state::sqlite_store) fn persist_linear_execution_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.linear_execution_events.values() {
		let payload_json = serde_json::to_string(&record.record)?;

		transaction.execute(
			"INSERT OR REPLACE INTO linear_execution_events (
					idempotency_key, service_id, issue_id, event_type, event_timestamp,
					event_unix, payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			persist::params![
				&record.record.idempotency_key,
				&record.record.service_id,
				&record.record.issue_id,
				&record.record.event_type,
				&record.record.event_timestamp,
				record.event_unix,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_private_execution_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in &state.private_execution_events {
		let payload_json = serde_json::to_string(&record.payload)?;

		transaction.execute(
			"INSERT OR REPLACE INTO private_execution_events (
					record_id, project_id, issue_id, run_id, attempt_number, event_type,
					payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			persist::params![
				record.record_id,
				&record.project_id,
				&record.issue_id,
				&record.run_id,
				record.attempt_number,
				&record.event_type,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;
	}

	Ok(())
}
