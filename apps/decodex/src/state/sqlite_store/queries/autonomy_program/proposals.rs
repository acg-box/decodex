use crate::{
	prelude::{Result, eyre},
	state::{
		StateData,
		runtime_records::AutonomyProposalRuntimeRecord,
		runtime_row_parsers,
		sqlite_store::{SqliteStateStore, queries::autonomy_program::load_errors},
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_autonomy_proposals(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 ORDER BY project_id ASC, objective_id ASC, objective_version ASC, updated_at_unix ASC",
		)?;
		let rows =
			statement.query_map([], runtime_row_parsers::autonomy_proposal_runtime_row_parts)?;

		for row in rows {
			let parts = row?;
			let record = match runtime_row_parsers::autonomy_proposal_record_from_row_parts(parts) {
				Ok(record) => record,
				Err(error)
					if load_errors::autonomy_proposal_load_error_is_quarantinable(
						error.as_ref(),
					) =>
				{
					tracing::warn!(
						error = %error,
						"Skipped invalid Autonomy Proposal during state snapshot load."
					);

					continue;
				},
				Err(error) => return Err(error),
			};

			state.autonomy_proposals.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_autonomy_proposals_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, proposal_id ASC",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id],
			runtime_row_parsers::autonomy_proposal_runtime_row_parts,
		)?;

		for row in rows {
			let parts = row?;
			let record = match runtime_row_parsers::autonomy_proposal_record_from_row_parts(parts) {
				Ok(record) => record,
				Err(error)
					if load_errors::autonomy_proposal_load_error_is_quarantinable(
						error.as_ref(),
					) =>
				{
					tracing::warn!(
						error = %error,
						project_id,
						"Skipped invalid Autonomy Proposal during project state load."
					);

					continue;
				},
				Err(error) => return Err(error),
			};

			state.autonomy_proposals.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn autonomy_proposal(
		&self,
		project_id: &str,
		proposal_id: &str,
	) -> Result<Option<AutonomyProposalRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 AND proposal_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(rusqlite::params![project_id, proposal_id])?;

		rows.next()?
			.map(runtime_row_parsers::autonomy_proposal_runtime_row_parts)
			.transpose()?
			.map(runtime_row_parsers::autonomy_proposal_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn autonomy_proposal_for_contract(
		&self,
		project_id: &str,
		contract_fingerprint_prefix: &str,
	) -> Result<Option<AutonomyProposalRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 AND substr(fingerprint, 1, 32) = ?2 \
			 ORDER BY proposal_id ASC LIMIT 2",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id, contract_fingerprint_prefix],
			runtime_row_parsers::autonomy_proposal_runtime_row_parts,
		)?;
		let records = rows
			.map(|row| runtime_row_parsers::autonomy_proposal_record_from_row_parts(row?))
			.collect::<Result<Vec<_>>>()?;

		if records.len() > 1 {
			eyre::bail!("Decision Contract matches multiple autonomy proposals.");
		}

		Ok(records.into_iter().next())
	}

	pub(in crate::state) fn recent_autonomy_proposals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyProposalRuntimeRecord>> {
		let limit = i64::try_from(limit).map_err(|_| {
			eyre::eyre!("Autonomy proposal readback limit exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, proposal_id ASC \
			 LIMIT ?2",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id, limit],
			runtime_row_parsers::autonomy_proposal_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			let parts = row?;
			let record = match runtime_row_parsers::autonomy_proposal_record_from_row_parts(parts) {
				Ok(record) => record,
				Err(error)
					if load_errors::autonomy_proposal_load_error_is_quarantinable(
						error.as_ref(),
					) =>
				{
					tracing::warn!(
						error = %error,
						project_id,
						"Skipped invalid Autonomy Proposal during recent project read."
					);

					continue;
				},
				Err(error) => return Err(error),
			};

			records.push(record);
		}

		Ok(records)
	}
}
