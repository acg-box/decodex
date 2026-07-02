use crate::{
	prelude::{Result, eyre},
	state::{
		StateData, runtime_records::AutonomySignalRuntimeRecord, runtime_row_parsers,
		sqlite_store::SqliteStateStore,
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_autonomy_signals(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 ORDER BY project_id ASC, objective_id ASC, objective_version ASC, updated_at_unix ASC",
		)?;
		let rows =
			statement.query_map([], runtime_row_parsers::autonomy_signal_runtime_row_parts)?;

		for row in rows {
			let record = runtime_row_parsers::autonomy_signal_record_from_row_parts(row?)?;

			state.autonomy_signals.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_autonomy_signals_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, signal_id ASC",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id],
			runtime_row_parsers::autonomy_signal_runtime_row_parts,
		)?;

		for row in rows {
			let record = runtime_row_parsers::autonomy_signal_record_from_row_parts(row?)?;

			state.autonomy_signals.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn autonomy_signal(
		&self,
		project_id: &str,
		signal_id: &str,
	) -> Result<Option<AutonomySignalRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 AND signal_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(rusqlite::params![project_id, signal_id])?;

		rows.next()?
			.map(runtime_row_parsers::autonomy_signal_runtime_row_parts)
			.transpose()?
			.map(runtime_row_parsers::autonomy_signal_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_autonomy_signals_for_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		objective_version: u64,
	) -> Result<Vec<AutonomySignalRuntimeRecord>> {
		let version = i64::try_from(objective_version).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 AND objective_id = ?2 AND objective_version = ?3 \
			 ORDER BY updated_at_unix ASC, signal_id ASC",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id, objective_id, version],
			runtime_row_parsers::autonomy_signal_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(runtime_row_parsers::autonomy_signal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn recent_autonomy_signals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomySignalRuntimeRecord>> {
		let limit = i64::try_from(limit).map_err(|_| {
			eyre::eyre!("Autonomy signal readback limit exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, signal_id ASC \
			 LIMIT ?2",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id, limit],
			runtime_row_parsers::autonomy_signal_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(runtime_row_parsers::autonomy_signal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}
}
