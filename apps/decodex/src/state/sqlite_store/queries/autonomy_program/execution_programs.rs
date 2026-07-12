use crate::{
	lane_authority::IntakeAuthority,
	prelude::Result,
	state::{
		StateData, runtime_records::ExecutionProgramRuntimeRecord, runtime_row_parsers,
		sqlite_store::SqliteStateStore,
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_intake_authorities(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_key, authority_id, payload_json FROM intake_authorities \
			 ORDER BY project_key, authority_id",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
		})?;
		for row in rows {
			let (project_key, authority_id, payload_json) = row?;
			let authority = serde_json::from_str::<IntakeAuthority>(&payload_json)?;
			authority.validate()?;
			if authority.project_key() != project_key || authority.authority_id() != authority_id {
				color_eyre::eyre::bail!(
					"Persisted Intake Authority key does not match its payload."
				);
			}
			state.intake_authorities.insert((project_key, authority_id), authority);
		}
		Ok(())
	}

	pub(in crate::state) fn load_execution_programs(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 ORDER BY project_id ASC, program_id ASC",
		)?;
		let rows =
			statement.query_map([], runtime_row_parsers::execution_program_runtime_row_parts)?;

		for row in rows {
			let record = runtime_row_parsers::execution_program_record_from_row_parts(row?)?;

			state.execution_programs.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn execution_program(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Option<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 AND program_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(rusqlite::params![project_id, program_id])?;

		rows.next()?
			.map(runtime_row_parsers::execution_program_runtime_row_parts)
			.transpose()?
			.map(runtime_row_parsers::execution_program_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_execution_programs_for_contract(
		&self,
		project_id: &str,
		source_contract_id: &str,
	) -> Result<Vec<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 AND source_contract_id = ?2 \
			 ORDER BY created_at_unix ASC, program_id ASC",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id, source_contract_id],
			runtime_row_parsers::execution_program_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(runtime_row_parsers::execution_program_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn list_execution_programs(
		&self,
		project_id: &str,
	) -> Result<Vec<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 \
			 ORDER BY created_at_unix ASC, program_id ASC",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id],
			runtime_row_parsers::execution_program_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(runtime_row_parsers::execution_program_record_from_row_parts(row?)?);
		}

		Ok(records)
	}
}
