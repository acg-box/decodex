use crate::state::{
	runtime_records::AutonomyRuntimePolicyRuntimeRecord,
	runtime_row_parsers,
	sqlite_store::persist::{self, Connection, Result, StateData, Transaction, eyre},
};

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
			persist::params![
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
			persist::params![
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
			persist::params![
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

pub(in crate::state::sqlite_store) fn persist_autonomy_runtime_policies(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.autonomy_runtime_policies.values() {
		upsert_autonomy_runtime_policy_record(transaction, record)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn upsert_autonomy_runtime_policy_record(
	connection: &Connection,
	record: &AutonomyRuntimePolicyRuntimeRecord,
) -> Result<AutonomyRuntimePolicyRuntimeRecord> {
	record.as_public().validate()?;

	let objective_version = i64::try_from(record.objective_version).map_err(|_| {
		eyre::eyre!("Autonomy runtime policy objective_version exceeds SQLite integer range.")
	})?;
	let public_non_goals_json = serde_json::to_string(&record.public_non_goals)?;

	connection.execute(
		"INSERT INTO autonomy_runtime_policies (
				project_id, policy_id, policy_version, objective_id, objective_version, objective_digest,
				authority_ref, accepted_by, accepted_at, acceptance_source,
				public_non_goals_json
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
		 ON CONFLICT(project_id, policy_id, policy_version) DO NOTHING",
		persist::params![
			&record.project_id,
			&record.policy_id,
			&record.policy_version,
			&record.objective_id,
			objective_version,
			&record.objective_digest,
			&record.authority_ref,
			&record.accepted_by,
			&record.accepted_at,
			&record.acceptance_source,
			public_non_goals_json,
		],
	)?;

	let parts = connection.query_row(
		"SELECT project_id, policy_id, policy_version, objective_id, objective_version, objective_digest,
				authority_ref, accepted_by, accepted_at, acceptance_source,
				public_non_goals_json
		 FROM autonomy_runtime_policies
		 WHERE project_id = ?1 AND policy_id = ?2 AND policy_version = ?3",
		persist::params![&record.project_id, &record.policy_id, &record.policy_version],
		runtime_row_parsers::autonomy_runtime_policy_runtime_row_parts,
	)?;
	let stored = runtime_row_parsers::autonomy_runtime_policy_record_from_row_parts(parts)?;

	stored.ensure_exact_replay(record)?;

	Ok(stored)
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
			persist::params![
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
			persist::params![
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
