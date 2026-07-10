use crate::{
	prelude::Result,
	state::{
		StateData, runtime_records::AutonomyRuntimePolicyRuntimeRecord, runtime_row_parsers,
		sqlite_store::SqliteStateStore,
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_autonomy_runtime_policies(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, policy_id, policy_version, objective_id, objective_version, objective_digest, \
			 authority_ref, accepted_by, accepted_at, acceptance_source, public_non_goals_json \
			 FROM autonomy_runtime_policies \
			 ORDER BY project_id ASC, policy_id ASC, policy_version ASC",
		)?;
		let rows = statement
			.query_map([], runtime_row_parsers::autonomy_runtime_policy_runtime_row_parts)?;

		for row in rows {
			let record = runtime_row_parsers::autonomy_runtime_policy_record_from_row_parts(row?)?;

			state.autonomy_runtime_policies.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn autonomy_runtime_policy(
		&self,
		project_id: &str,
		policy_id: &str,
		policy_version: &str,
	) -> Result<Option<AutonomyRuntimePolicyRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, policy_id, policy_version, objective_id, objective_version, objective_digest, \
			 authority_ref, accepted_by, accepted_at, acceptance_source, public_non_goals_json \
			 FROM autonomy_runtime_policies \
			 WHERE project_id = ?1 AND policy_id = ?2 AND policy_version = ?3 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(rusqlite::params![project_id, policy_id, policy_version])?;

		rows.next()?
			.map(runtime_row_parsers::autonomy_runtime_policy_runtime_row_parts)
			.transpose()?
			.map(runtime_row_parsers::autonomy_runtime_policy_record_from_row_parts)
			.transpose()
	}
}
