use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn protocol_event_replay_is_idempotent_when_payload_matches() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let payload = r#"{"threadId":"thread-1","attemptNumber":4}"#;

	store
		.append_event("run-1", 32, "thread/archive", payload)
		.expect("first archive event should append");
	store
		.append_event("run-1", 32, "thread/archive", payload)
		.expect("matching archive replay should be idempotent");

	assert_eq!(store.event_count("run-1").expect("event count should load"), 1);

	let connection = Connection::open(&state_path).expect("sqlite should open");
	let payload_sha256: String = connection
		.query_row(
			"SELECT payload_sha256 FROM protocol_events WHERE run_id = 'run-1' AND sequence_number = 32",
			[],
			|row| row.get(0),
		)
		.expect("payload digest should read");

	assert_eq!(payload_sha256.len(), 64);
	assert!(payload_sha256.chars().all(|character| character.is_ascii_hexdigit()));
}
