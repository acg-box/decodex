use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{
		self, OptionalExtension, ProtocolEventSummaryRecord, Result, StateData,
		run_activity_summary_record_from_row,
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_protocol_event_summaries(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		self.load_compacted_protocol_event_summaries(state)
	}

	pub(in crate::state) fn load_protocol_event_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			state.event_summaries.remove(run_id);

			if !self.load_compacted_protocol_event_summary_for_run(state, run_id)? {
				self.load_protocol_event_summary_for_run(state, run_id)?;
			}
		}

		Ok(())
	}

	pub(in crate::state) fn rebuild_protocol_event_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			state.event_summaries.remove(run_id);
			self.load_protocol_event_summary_for_run(state, run_id)?;
		}

		Ok(())
	}

	pub(in crate::state) fn load_protocol_event_summary_for_run(
		&self,
		state: &mut StateData,
		run_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT totals.event_count, totals.last_sequence_number, last.event_type, \
			 last.created_at, last.created_at_unix \
			 FROM (
			 SELECT COUNT(*) AS event_count, MAX(sequence_number) AS last_sequence_number \
			 FROM protocol_events WHERE run_id = ?1
			 ) totals \
			 JOIN protocol_events last \
			 ON last.run_id = ?1 \
			 AND last.sequence_number = totals.last_sequence_number",
		)?;
		let summary = statement
			.query_row(queries::params![run_id], |row| {
				Ok(ProtocolEventSummaryRecord {
					event_count: row.get(0)?,
					last_sequence_number: Some(row.get(1)?),
					last_event_type: Some(row.get(2)?),
					last_event_at: Some(row.get(3)?),
					last_event_at_unix: Some(row.get(4)?),
				})
			})
			.optional()?;

		if let Some(summary) = summary {
			self.upsert_protocol_event_summary(run_id, &summary)?;
			state.event_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}

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

	pub(in crate::state) fn load_compacted_protocol_event_summaries(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, event_count, last_sequence_number, last_event_type, last_event_at, \
			 last_event_at_unix FROM protocol_event_summaries ORDER BY run_id",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, String>(0)?,
				ProtocolEventSummaryRecord {
					event_count: row.get(1)?,
					last_sequence_number: row.get(2)?,
					last_event_type: row.get(3)?,
					last_event_at: row.get(4)?,
					last_event_at_unix: row.get(5)?,
				},
			))
		})?;

		for row in rows {
			let (run_id, summary) = row?;

			state.event_summaries.insert(run_id, summary);
		}

		Ok(())
	}

	pub(in crate::state) fn load_compacted_protocol_event_summary_for_run(
		&self,
		state: &mut StateData,
		run_id: &str,
	) -> Result<bool> {
		let mut statement = self.connection.prepare(
			"SELECT event_count, last_sequence_number, last_event_type, last_event_at, \
			 last_event_at_unix FROM protocol_event_summaries WHERE run_id = ?1",
		)?;
		let summary = statement
			.query_row(queries::params![run_id], |row| {
				Ok(ProtocolEventSummaryRecord {
					event_count: row.get(0)?,
					last_sequence_number: row.get(1)?,
					last_event_type: row.get(2)?,
					last_event_at: row.get(3)?,
					last_event_at_unix: row.get(4)?,
				})
			})
			.optional()?;

		if let Some(summary) = summary {
			state.event_summaries.insert(run_id.to_owned(), summary);

			return Ok(true);
		}

		Ok(false)
	}

	pub(in crate::state) fn upsert_protocol_event_summary(
		&self,
		run_id: &str,
		summary: &ProtocolEventSummaryRecord,
	) -> Result<()> {
		let now = queries::timestamp_parts();

		self.connection.execute(
			"INSERT OR REPLACE INTO protocol_event_summaries (
					run_id, event_count, last_sequence_number, last_event_type, last_event_at,
					last_event_at_unix, compacted_at, compacted_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			queries::params![
				run_id,
				summary.event_count,
				summary.last_sequence_number,
				summary.last_event_type.as_deref(),
				summary.last_event_at.as_deref(),
				summary.last_event_at_unix,
				now.text,
				now.unix,
			],
		)?;

		Ok(())
	}
}
