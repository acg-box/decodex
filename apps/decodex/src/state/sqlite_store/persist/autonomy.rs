use super::{Result, StateData, Transaction, eyre, params};

pub(in crate::state::sqlite_store) fn persist_decision_contracts(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.decision_contracts.values() {
		let payload_json = serde_json::to_string(&record.contract)?;

		transaction.execute(
			"INSERT OR REPLACE INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_autonomy_objectives(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.autonomy_objectives.values() {
		let payload_json = serde_json::to_string(&record.objective)?;
		let version = i64::try_from(record.objective.version())
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_objectives (
					project_id, objective_id, version, state, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_autonomy_signals(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.autonomy_signals.values() {
		let payload_json = serde_json::to_string(&record.signal)?;
		let version = i64::try_from(record.signal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_signals (
					project_id, signal_id, objective_id, objective_version, kind, fingerprint,
					freshness, evidence_class, confidence, privacy, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_autonomy_proposals(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.autonomy_proposals.values() {
		let payload_json = serde_json::to_string(&record.proposal)?;
		let version = i64::try_from(record.proposal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_execution_programs(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.execution_programs.values() {
		let payload_json = serde_json::to_string(&record.program)?;

		transaction.execute(
			"INSERT OR REPLACE INTO execution_programs (
					project_id, program_id, source_contract_id, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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
	}

	Ok(())
}
