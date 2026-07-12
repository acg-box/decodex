use crate::state::sqlite_store::schema::{Result, SqliteStateStore};

impl SqliteStateStore {
	pub(in crate::state) fn bootstrap_authority_event_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS authority_event_chain_head (
	singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
	generation INTEGER NOT NULL,
	sequence INTEGER NOT NULL,
	genesis_hash BLOB NOT NULL CHECK (length(genesis_hash) = 32),
	event_hash BLOB NOT NULL CHECK (length(event_hash) = 32)
);
CREATE TABLE IF NOT EXISTS authority_events (
	generation INTEGER NOT NULL,
	sequence INTEGER NOT NULL,
	event_id TEXT NOT NULL UNIQUE,
	previous_event_hash BLOB NOT NULL CHECK (length(previous_event_hash) = 32),
	event_hash BLOB NOT NULL CHECK (length(event_hash) = 32),
	event_cbor BLOB NOT NULL,
	recorded_at_unix_micros INTEGER NOT NULL,
	PRIMARY KEY (generation, sequence)
);
CREATE INDEX IF NOT EXISTS authority_events_event_type_time_idx
ON authority_events (recorded_at_unix_micros, event_id);
"#,
		)?;
		Ok(())
	}
}
