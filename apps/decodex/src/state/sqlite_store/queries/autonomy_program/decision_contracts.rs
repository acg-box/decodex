use crate::{
	prelude::Result,
	state::{
		StateData,
		runtime_records::DecisionContractRuntimeRecord,
		runtime_row_parsers,
		sqlite_store::{SqliteStateStore, queries::autonomy_program::load_errors},
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_decision_contracts(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 ORDER BY project_id ASC, contract_id ASC",
		)?;
		let rows =
			statement.query_map([], runtime_row_parsers::decision_contract_runtime_row_parts)?;

		for row in rows {
			let parts = row?;
			let record = match runtime_row_parsers::decision_contract_record_from_row_parts(parts) {
				Ok(record) => record,
				Err(error)
					if load_errors::decision_contract_load_error_is_quarantinable(
						error.as_ref(),
					) =>
				{
					tracing::warn!(
						error = %error,
						"Skipped invalid Decision Contract during state snapshot load."
					);

					continue;
				},
				Err(error) => return Err(error),
			};

			state.decision_contracts.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 AND contract_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(rusqlite::params![project_id, contract_id])?;

		rows.next()?
			.map(runtime_row_parsers::decision_contract_runtime_row_parts)
			.transpose()?
			.map(runtime_row_parsers::decision_contract_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_decision_contracts_for_issue(
		&self,
		project_id: &str,
		source_issue_id: &str,
	) -> Result<Vec<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 AND source_issue_id = ?2 \
			 ORDER BY created_at_unix ASC, contract_id ASC",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id, source_issue_id],
			runtime_row_parsers::decision_contract_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			let parts = row?;
			let record = match runtime_row_parsers::decision_contract_record_from_row_parts(parts) {
				Ok(record) => record,
				Err(error)
					if load_errors::decision_contract_load_error_is_quarantinable(
						error.as_ref(),
					) =>
				{
					tracing::warn!(
						error = %error,
						project_id,
						source_issue_id,
						"Skipped invalid Decision Contract during issue list read."
					);

					continue;
				},
				Err(error) => return Err(error),
			};

			records.push(record);
		}

		Ok(records)
	}

	pub(in crate::state) fn list_decision_contracts_for_project(
		&self,
		project_id: &str,
	) -> Result<Vec<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 \
			 ORDER BY created_at_unix ASC, contract_id ASC",
		)?;
		let rows = statement.query_map(
			rusqlite::params![project_id],
			runtime_row_parsers::decision_contract_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			let parts = row?;
			let record = match runtime_row_parsers::decision_contract_record_from_row_parts(parts) {
				Ok(record) => record,
				Err(error)
					if load_errors::decision_contract_load_error_is_quarantinable(
						error.as_ref(),
					) =>
				{
					tracing::warn!(
						error = %error,
						project_id,
						"Skipped invalid Decision Contract during project list read."
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
