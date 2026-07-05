use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn persistent_open_backfills_protocol_event_summaries_from_legacy_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");

	{
		let store = StateStore::open(&state_path).expect("state store should create schema");

		store.record_run_attempt("run-legacy", "PUB-101", 1, "running").expect("run should record");
	}

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute("DELETE FROM schema_meta WHERE key = 'schema_version'", [])
		.expect("schema version should reset");
	connection
		.execute(
			"DELETE FROM schema_meta
			 WHERE key = 'migration:protocol_event_summaries_from_events:v12'",
			[],
		)
		.expect("protocol summary migration marker should reset");
	connection
		.execute("DELETE FROM protocol_event_summaries", [])
		.expect("summary rows should clear");
	connection
		.execute(
			"INSERT INTO protocol_events (
					run_id, sequence_number, event_type, payload_sha256, created_at, created_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			rusqlite::params![
				"run-legacy",
				1_i64,
				"turn/started",
				"sha-1",
				"2026-06-17T00:00:00Z",
				1_i64,
			],
		)
		.expect("legacy event should insert");
	connection
		.execute(
			"INSERT INTO protocol_events (
					run_id, sequence_number, event_type, payload_sha256, created_at, created_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			rusqlite::params![
				"run-legacy",
				2_i64,
				"turn/completed",
				"sha-2",
				"2026-06-17T00:00:01Z",
				2_i64,
			],
		)
		.expect("legacy event should insert");

	let reopened = StateStore::open(&state_path).expect("state store should migrate");

	assert_eq!(reopened.event_count("run-legacy").expect("event count should load"), 2);

	let summary = connection
		.query_row(
			"SELECT event_count, last_sequence_number, last_event_type
			 FROM protocol_event_summaries
			 WHERE run_id = 'run-legacy'",
			[],
			|row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)),
		)
		.expect("summary should persist");

	assert_eq!(summary, (2, 2, String::from("turn/completed")));
}
