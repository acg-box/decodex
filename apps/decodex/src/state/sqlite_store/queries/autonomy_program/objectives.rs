use crate::{
	prelude::{Result, eyre},
	state::{
		StateData, runtime_records::AutonomyObjectiveRuntimeRecord, runtime_row_parsers,
		sqlite_store::SqliteStateStore,
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_autonomy_objectives(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 ORDER BY project_id ASC, objective_id ASC, version ASC",
		)?;
		let rows =
			statement.query_map([], runtime_row_parsers::autonomy_objective_runtime_row_parts)?;

		for row in rows {
			let record = runtime_row_parsers::autonomy_objective_record_from_row_parts(row?)?;

			state.autonomy_objectives.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
	) -> Result<Option<AutonomyObjectiveRuntimeRecord>> {
		let version = i64::try_from(version)
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 AND version = ?3 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(rusqlite::params![project_id, objective_id, version])?;

		rows.next()?
			.map(runtime_row_parsers::autonomy_objective_runtime_row_parts)
			.transpose()?
			.map(runtime_row_parsers::autonomy_objective_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn current_accepted_autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Option<AutonomyObjectiveRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 AND state = 'accepted' \
			 ORDER BY version DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(rusqlite::params![project_id, objective_id])?;

		rows.next()?
			.map(runtime_row_parsers::autonomy_objective_runtime_row_parts)
			.transpose()?
			.map(runtime_row_parsers::autonomy_objective_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_autonomy_objective_history(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Vec<AutonomyObjectiveRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 \
			 ORDER BY version ASC",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id, objective_id],
			runtime_row_parsers::autonomy_objective_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(runtime_row_parsers::autonomy_objective_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn recent_autonomy_objectives_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyObjectiveRuntimeRecord>> {
		let limit = i64::try_from(limit).unwrap_or(i64::MAX);
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, objective_id ASC, version ASC \
			 LIMIT ?2",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id, limit],
			runtime_row_parsers::autonomy_objective_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(runtime_row_parsers::autonomy_objective_record_from_row_parts(row?)?);
		}

		Ok(records)
	}
}
