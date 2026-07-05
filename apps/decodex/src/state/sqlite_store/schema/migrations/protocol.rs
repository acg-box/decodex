use crate::state::sqlite_store::schema::{self, Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn backfill_protocol_event_summaries_from_events(&self) -> Result<()> {
		let now = schema::timestamp_parts();

		self.connection.execute(
			"INSERT INTO protocol_event_summaries (
					run_id, event_count, last_sequence_number, last_event_type, last_event_at,
					last_event_at_unix, compacted_at, compacted_at_unix
				)
			 SELECT totals.run_id, totals.event_count, totals.last_sequence_number,
					last.event_type, last.created_at, last.created_at_unix, ?1, ?2
			 FROM (
				 SELECT run_id, COUNT(*) AS event_count, MAX(sequence_number) AS last_sequence_number
				 FROM protocol_events
				 GROUP BY run_id
			 ) totals
			 JOIN protocol_events last
			 ON last.run_id = totals.run_id
			 AND last.sequence_number = totals.last_sequence_number
			 ON CONFLICT(run_id) DO UPDATE SET
				 event_count = excluded.event_count,
				 last_sequence_number = excluded.last_sequence_number,
				 last_event_type = excluded.last_event_type,
				 last_event_at = excluded.last_event_at,
				 last_event_at_unix = excluded.last_event_at_unix,
				 compacted_at = excluded.compacted_at,
				 compacted_at_unix = excluded.compacted_at_unix",
			schema::params![now.text, now.unix],
		)?;

		Ok(())
	}
}
