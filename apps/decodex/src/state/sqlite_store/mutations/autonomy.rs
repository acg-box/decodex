#[allow(clippy::wildcard_imports)] use super::*;

impl SqliteStateStore {
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
			params![
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
			params![
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

	pub(in crate::state) fn upsert_autonomy_signal(
		&self,
		record: &AutonomySignalRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.signal)?;
		let version = i64::try_from(record.signal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;

		self.connection.execute(
			"INSERT INTO autonomy_signals (
					project_id, signal_id, objective_id, objective_version, kind, fingerprint,
					freshness, evidence_class, confidence, privacy, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
			 ON CONFLICT(project_id, signal_id) DO UPDATE SET
				 objective_id = excluded.objective_id,
				 objective_version = excluded.objective_version,
				 kind = excluded.kind,
				 fingerprint = excluded.fingerprint,
				 freshness = excluded.freshness,
				 evidence_class = excluded.evidence_class,
				 confidence = excluded.confidence,
				 privacy = excluded.privacy,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.signal.id(),
				record.signal.objective_id(),
				version,
				record.signal.kind().as_str(),
				record.signal.fingerprint(),
				record.signal.freshness().as_str(),
				record.signal.evidence_class().as_str(),
				record.signal.confidence().as_str(),
				record.signal.privacy().as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn upsert_autonomy_proposal(
		&self,
		record: &AutonomyProposalRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.proposal)?;
		let version = i64::try_from(record.proposal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;

		self.connection.execute(
			"INSERT INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
			 ON CONFLICT(project_id, proposal_id) DO UPDATE SET
				 objective_id = excluded.objective_id,
				 objective_version = excluded.objective_version,
				 state = excluded.state,
				 fingerprint = excluded.fingerprint,
				 source_family = excluded.source_family,
				 intended_surface = excluded.intended_surface,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.proposal.id(),
				record.proposal.objective_id(),
				version,
				record.state.as_str(),
				record.proposal.fingerprint(),
				record.proposal.source_family(),
				record.proposal.intended_surface(),
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
			params![
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
			params![&record.project_id, record.program.program_id()],
		)?;
		self.connection.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1 AND program_id = ?2",
			params![&record.project_id, record.program.program_id()],
		)?;

		persist::insert_program_intake_state(&self.connection, record)
	}
}
