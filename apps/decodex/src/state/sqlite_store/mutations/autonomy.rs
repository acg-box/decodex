mod signals;

use crate::{
	autonomy_runtime_policy,
	state::{
		AutonomyRuntimePolicyReceiptInput, AutonomyRuntimePolicyRecord,
		sqlite_store::mutations::{
			self, AutonomyObjectiveRuntimeRecord, AutonomyRuntimePolicyRuntimeRecord,
			DecisionContractRuntimeRecord, ExecutionProgramRuntimeRecord, Result, SqliteStateStore,
			eyre, persist,
		},
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn issue_autonomy_runtime_policy_receipt(
		&self,
		input: AutonomyRuntimePolicyReceiptInput<'_>,
	) -> Result<()> {
		self.connection.execute(
			"INSERT INTO autonomy_runtime_policy_receipts (
				project_id, receipt_id, principal, candidate_digest, candidate_json,
				created_at, expires_at_unix, consumed_at
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
			mutations::params![
				input.project_id,
				input.receipt_id,
				input.principal,
				input.candidate_digest,
				serde_json::to_string(input.candidate)?,
				input.created_at,
				input.expires_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn consume_autonomy_runtime_policy_receipt(
		&mut self,
		project_id: &str,
		receipt_id: &str,
		principal: &str,
		now: &str,
		now_unix: i64,
	) -> Result<AutonomyRuntimePolicyRuntimeRecord> {
		let transaction = self.connection.transaction()?;
		let (stored_principal, candidate_digest, candidate_json, expires_at_unix, consumed_at) =
			transaction.query_row(
				"SELECT principal, candidate_digest, candidate_json, expires_at_unix, consumed_at
				 FROM autonomy_runtime_policy_receipts
				 WHERE project_id = ?1 AND receipt_id = ?2",
				mutations::params![project_id, receipt_id],
				|row| {
					Ok((
						row.get::<_, String>(0)?,
						row.get::<_, String>(1)?,
						row.get::<_, String>(2)?,
						row.get::<_, i64>(3)?,
						row.get::<_, Option<String>>(4)?,
					))
				},
			)?;

		if stored_principal != principal {
			eyre::bail!("runtime_policy_receipt_principal_mismatch");
		}
		if consumed_at.is_some() {
			eyre::bail!("runtime_policy_receipt_already_consumed");
		}
		if expires_at_unix < now_unix {
			eyre::bail!("runtime_policy_receipt_expired");
		}
		if expires_at_unix - now_unix > 600 {
			eyre::bail!("runtime_policy_receipt_expiry_invalid");
		}

		let candidate: AutonomyRuntimePolicyRecord = serde_json::from_str(&candidate_json)?;

		candidate.validate()?;

		let calculated_digest =
			autonomy_runtime_policy::runtime_policy_candidate_digest(&candidate)?;

		if candidate.project_id() != project_id || calculated_digest != candidate_digest {
			eyre::bail!("runtime_policy_receipt_candidate_mismatch");
		}

		let runtime_record = AutonomyRuntimePolicyRuntimeRecord::from(candidate);
		let stored = persist::upsert_autonomy_runtime_policy_record(&transaction, &runtime_record)?;
		let consumed = transaction.execute(
			"UPDATE autonomy_runtime_policy_receipts SET consumed_at = ?3
			 WHERE project_id = ?1 AND receipt_id = ?2 AND consumed_at IS NULL",
			mutations::params![project_id, receipt_id, now],
		)?;

		if consumed != 1 {
			eyre::bail!("runtime_policy_receipt_consumption_conflict");
		}

		transaction.commit()?;

		Ok(stored)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_decision_contract(
		&self,
		record: &DecisionContractRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.contract)?;

		self.connection.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
			 ON CONFLICT(project_id, contract_id) DO UPDATE SET
				 source_issue_id = excluded.source_issue_id,
				 status = excluded.status,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			mutations::params![
				&record.project_id,
				record.contract.contract_id(),
				record.source_issue_id.as_deref(),
				record.status.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_objective(
		&self,
		record: &AutonomyObjectiveRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.objective)?;
		let version = i64::try_from(record.objective.version())
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;

		self.connection.execute(
			"INSERT INTO autonomy_objectives (
					project_id, objective_id, version, state, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
			 ON CONFLICT(project_id, objective_id, version) DO UPDATE SET
				 state = excluded.state,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			mutations::params![
				&record.project_id,
				record.objective.id(),
				version,
				record.state.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn upsert_autonomy_runtime_policy(
		&self,
		record: &AutonomyRuntimePolicyRuntimeRecord,
	) -> Result<AutonomyRuntimePolicyRuntimeRecord> {
		persist::upsert_autonomy_runtime_policy_record(&self.connection, record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_execution_program(
		&self,
		record: &ExecutionProgramRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.program)?;

		self.connection.execute(
			"INSERT INTO execution_programs (
					project_id, program_id, source_contract_id, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
			 ON CONFLICT(project_id, program_id) DO UPDATE SET
				 source_contract_id = excluded.source_contract_id,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			mutations::params![
				&record.project_id,
				record.program.program_id(),
				record.source_contract_id.as_deref(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
		self.replace_program_intake_state(record)?;

		Ok(())
	}

	pub(in crate::state) fn replace_program_intake_state(
		&self,
		record: &ExecutionProgramRuntimeRecord,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM program_intake_plans WHERE project_id = ?1 AND program_id = ?2",
			mutations::params![&record.project_id, record.program.program_id()],
		)?;
		self.connection.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1 AND program_id = ?2",
			mutations::params![&record.project_id, record.program.program_id()],
		)?;

		persist::insert_program_intake_state(&self.connection, record)
	}
}
