mod autonomy;
mod cleanup;
mod lanes;
mod project;
mod runs;

pub(super) use rusqlite::params;

use crate::state::sqlite_store::{
	AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord,
	AutonomyRuntimePolicyRuntimeRecord, AutonomySignalRuntimeRecord, ChildAgentActivitySummary,
	ConnectorBackoff, DecisionContractRuntimeRecord, ExecutionProgramRuntimeRecord, IssueLease,
	LinearExecutionEventRuntimeRecord, OptionalExtension, PrivateExecutionEventRuntimeRecord,
	ProjectRegistration, ProtocolEventRecord, Result, RunActivitySummaryRecord, RunAttemptRecord,
	RunControlChannelRecord, SqliteStateStore, StateData, WorktreeMappingRecord,
	connector_backoff_from_row, eyre, persist, protocol_event_record_from_row,
};

impl SqliteStateStore {
	pub(in crate::state) fn begin_program_intake_attempt(
		&self,
		project_id: &str,
		contract_id: &str,
		canonical_key: &str,
		request_digest: &str,
		occurred_at: &str,
	) -> Result<&'static str> {
		let inserted = self.connection.execute(
			"INSERT OR IGNORE INTO program_intake_attempts (
				project_id, contract_id, canonical_key, request_digest, status, created_at, updated_at
			) VALUES (?1, ?2, ?3, ?4, 'prepared', ?5, ?5)",
			params![project_id, contract_id, canonical_key, request_digest, occurred_at],
		)?;

		if inserted == 1 {
			return Ok("acquired");
		}

		let (stored_key, stored_digest, status) = self.connection.query_row(
			"SELECT canonical_key, request_digest, status FROM program_intake_attempts
			 WHERE project_id = ?1 AND contract_id = ?2",
			params![project_id, contract_id],
			|row| {
				Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
			},
		)?;

		if stored_key != canonical_key || stored_digest != request_digest {
			eyre::bail!("program_intake_attempt_request_mismatch");
		}

		match status.as_str() {
			"prepared" => Ok("prepared"),
			"started" => Ok("started"),
			"completed" => Ok("completed"),
			_ => eyre::bail!("Program Intake attempt has unsupported state."),
		}
	}

	pub(in crate::state) fn program_intake_attempt_status(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<String>> {
		self.connection
			.query_row(
				"SELECT status FROM program_intake_attempts
				 WHERE project_id = ?1 AND contract_id = ?2",
				params![project_id, contract_id],
				|row| row.get(0),
			)
			.optional()
			.map_err(Into::into)
	}

	pub(in crate::state) fn mark_program_intake_attempt_started(
		&self,
		project_id: &str,
		contract_id: &str,
		occurred_at: &str,
	) -> Result<()> {
		let updated = self.connection.execute(
			"UPDATE program_intake_attempts SET status = 'started', updated_at = ?3
			 WHERE project_id = ?1 AND contract_id = ?2 AND status = 'prepared'",
			params![project_id, contract_id, occurred_at],
		)?;

		if updated != 1 {
			eyre::bail!("Program Intake attempt is not retry-safe prepared state.");
		}

		Ok(())
	}

	pub(in crate::state) fn complete_program_intake_attempt(
		&self,
		project_id: &str,
		contract_id: &str,
		occurred_at: &str,
	) -> Result<()> {
		let updated = self.connection.execute(
			"UPDATE program_intake_attempts SET status = 'completed', updated_at = ?3
			 WHERE project_id = ?1 AND contract_id = ?2
			 AND status IN ('started', 'completed')",
			params![project_id, contract_id, occurred_at],
		)?;

		if updated != 1 {
			eyre::bail!("Program Intake attempt claim does not exist.");
		}

		Ok(())
	}

	pub(in crate::state) fn persist_runtime_state(&mut self, state: &StateData) -> Result<()> {
		let transaction = self.connection.transaction()?;

		persist::persist_projects(&transaction, state)?;
		persist::persist_leases(&transaction, state)?;
		persist::persist_run_attempts(&transaction, state)?;
		persist::persist_run_control_channels(&transaction, state)?;
		persist::persist_protocol_events(&transaction, state)?;
		persist::persist_run_activity_summaries(&transaction, state)?;
		persist::persist_worktrees(&transaction, state)?;
		persist::persist_linear_execution_events(&transaction, state)?;
		persist::persist_private_execution_events(&transaction, state)?;
		persist::persist_decision_contracts(&transaction, state)?;
		persist::persist_autonomy_objectives(&transaction, state)?;
		persist::persist_autonomy_runtime_policies(&transaction, state)?;
		persist::persist_autonomy_signals(&transaction, state)?;
		persist::persist_autonomy_proposals(&transaction, state)?;
		persist::persist_execution_programs(&transaction, state)?;
		persist::persist_intake_authorities(&transaction, state)?;
		persist::persist_program_intake_state(&transaction, state)?;
		persist::persist_review_lifecycle_records(&transaction, state)?;
		persist::persist_review_policy_checkpoints(&transaction, state)?;
		persist::persist_evidence_artifacts(&transaction, state)?;
		persist::persist_loop_guardrail_checkpoints(&transaction, state)?;
		persist::persist_connector_backoffs(&transaction, state)?;

		transaction.commit()?;

		Ok(())
	}
}
