use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, OptionalExtension, Result, StateData, run_activity_summary_record_from_row},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_run_activity_summaries(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
			 updated_at, updated_at_unix FROM run_activity_summaries ORDER BY run_id",
		)?;
		let rows = statement.query_map([], run_activity_summary_record_from_row)?;

		for row in rows {
			let summary = row?;

			state.run_activity_summaries.insert(summary.run_id.clone(), summary);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_activity_summaries_for_loaded_runs(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let run_ids = state.run_attempts.keys().cloned().collect::<Vec<_>>();

		self.load_run_activity_summaries_for_runs(state, &run_ids)
	}

	pub(in crate::state) fn load_run_activity_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			self.load_run_activity_summary_for_run(state, run_id)?;
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_activity_summary_for_run(
		&self,
		state: &mut StateData,
		run_id: &str,
	) -> Result<()> {
		state.run_activity_summaries.remove(run_id);

		let mut statement = self.connection.prepare(
			"SELECT run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
			 updated_at, updated_at_unix FROM run_activity_summaries WHERE run_id = ?1",
		)?;
		let summary = statement
			.query_row(queries::params![run_id], run_activity_summary_record_from_row)
			.optional()?;

		if let Some(summary) = summary {
			state.run_activity_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}
}
