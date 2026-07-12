use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn persistent_open_rejects_future_schema_version() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");

	StateStore::open(&state_path).expect("state store should create schema");

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute("UPDATE schema_meta SET value = '18' WHERE key = 'schema_version'", [])
		.expect("future schema version should set");

	let error = match StateStore::open(&state_path) {
		Ok(_) => panic!("older binary must reject future schema"),
		Err(error) => error,
	};
	assert!(error.to_string().contains("newer Decodex binary"));

	let version: String = connection
		.query_row("SELECT value FROM schema_meta WHERE key = 'schema_version'", [], |row| {
			row.get(0)
		})
		.expect("schema version should read");

	assert_eq!(version, "18");
}
