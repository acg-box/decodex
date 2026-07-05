use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{
	StateStore,
	tests::{self},
};

#[test]
fn persistent_open_keeps_protocol_backfill_marker_when_later_migration_fails() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");

	{
		let store = StateStore::open(&state_path).expect("state store should create schema");

		store.record_run_attempt("run-legacy", "PUB-102", 1, "running").expect("run should record");
	}

	let connection = Connection::open(&state_path).expect("sqlite should open");
	let mut removed_field_payload = serde_json::to_value(tests::latent_decision_contract_fixture())
		.expect("fixture should encode as JSON");

	removed_field_payload["contract_id"] = serde_json::json!("removed-field-invalid-contract");
	removed_field_payload["status"] = serde_json::json!("accepted_promoted");
	removed_field_payload["execution_readiness"]["ready_for_issue_shaping"] =
		serde_json::json!(false);

	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Removed-field invalid summary."]),
	);
	connection
		.execute("UPDATE schema_meta SET value = '11' WHERE key = 'schema_version'", [])
		.expect("schema version should mark removed-field state");
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
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				"decodex",
				"removed-field-invalid-contract",
				"XY-BAD",
				"accepted_promoted",
				serde_json::to_string(&removed_field_payload)
					.expect("removed-field payload should serialize"),
				"2026-06-17T00:00:00Z",
				1_i64,
				"2026-06-17T00:00:00Z",
				1_i64,
			],
		)
		.expect("invalid removed-field decision contract row should insert");

	assert!(
		StateStore::open(&state_path).is_err(),
		"invalid removed-field decision contract migration should still fail closed"
	);

	let marker: String = connection
		.query_row(
			"SELECT value FROM schema_meta
			 WHERE key = 'migration:protocol_event_summaries_from_events:v12'",
			[],
			|row| row.get(0),
		)
		.expect("protocol backfill marker should persist after later migration failure");
	let event_count: i64 = connection
		.query_row(
			"SELECT event_count FROM protocol_event_summaries WHERE run_id = 'run-legacy'",
			[],
			|row| row.get(0),
		)
		.expect("protocol summary should persist after later migration failure");

	assert_eq!(marker, "completed");
	assert_eq!(event_count, 1);
}
