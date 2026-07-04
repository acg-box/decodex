use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, Result},
};

impl SqliteStateStore {
	pub(in crate::state) fn run_has_protocol_event(
		&self,
		run_id: &str,
		event_type: &str,
	) -> Result<bool> {
		let exists = self.connection.query_row(
			"SELECT EXISTS(
			 SELECT 1 FROM protocol_events
			 WHERE run_id = ?1 AND event_type = ?2
			 LIMIT 1
			 )",
			queries::params![run_id, event_type],
			|row| row.get::<_, i64>(0),
		)?;

		Ok(exists != 0)
	}
}
