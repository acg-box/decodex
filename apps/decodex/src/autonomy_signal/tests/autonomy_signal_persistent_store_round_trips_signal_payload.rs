use rusqlite::Connection;
use serde_json::Value;

use crate::{
	autonomy_signal::{
		AutonomySignal,
		tests::{self},
	},
	state::StateStore,
};

#[test]
fn autonomy_signal_persistent_store_round_trips_signal_payload() {
	let tempdir = tempfile::tempdir().expect("tempdir should create");
	let db_path = tempdir.path().join("runtime.sqlite3");
	let signal = {
		let store = StateStore::open(&db_path).expect("store should open");

		tests::accept_objective(&store, 1);

		let signal = AutonomySignal::runtime_health(tests::signal_input())
			.expect("runtime signal should validate");

		store.record_autonomy_signal("decodex", signal.clone()).expect("signal should store");

		signal
	};
	let reopened = StateStore::open(&db_path).expect("store should reopen");
	let stored = reopened
		.autonomy_signal("decodex", signal.id())
		.expect("signal read should succeed")
		.expect("signal should exist");

	assert_eq!(stored.signal(), &signal);
	assert_eq!(stored.signal().source_refs(), ["status:XY-1085:runtime-health"]);
	assert!(stored.signal().primary_source_refs().is_empty());
}

#[test]
fn autonomy_signal_store_reads_and_migrates_legacy_docs_skill_drift_kind() {
	let tempdir = tempfile::tempdir().expect("tempdir should create");
	let db_path = tempdir.path().join("runtime.sqlite3");
	let signal = {
		let store = StateStore::open(&db_path).expect("store should open");

		tests::accept_objective(&store, 1);

		let signal = AutonomySignal::docs_plugin_drift(tests::signal_input())
			.expect("docs plugin signal should validate");

		store.record_autonomy_signal("decodex", signal.clone()).expect("signal should store");

		signal
	};
	let (legacy_id, legacy_fingerprint, legacy_payload) = legacy_docs_skill_drift_payload(&signal);
	let connection = Connection::open(&db_path).expect("db should open");

	connection
		.execute(
			"UPDATE autonomy_signals
			 SET signal_id = ?3,
			     kind = 'docs_skill_drift',
			     fingerprint = ?4,
			     payload_json = ?5
			 WHERE project_id = ?1 AND signal_id = ?2",
			rusqlite::params![
				"decodex",
				signal.id(),
				legacy_id,
				legacy_fingerprint,
				legacy_payload
			],
		)
		.expect("legacy row should update");

	let reopened = StateStore::open(&db_path).expect("store should reopen");
	let stored = reopened
		.autonomy_signal("decodex", signal.id())
		.expect("legacy signal read should succeed")
		.expect("legacy signal should exist");
	let migrated_legacy_lookup = reopened
		.autonomy_signal("decodex", &legacy_id)
		.expect("migrated legacy lookup should succeed");

	assert_eq!(stored.signal().kind().as_str(), "docs_plugin_drift");
	assert_eq!(stored.signal().id(), signal.id());
	assert_eq!(stored.signal().fingerprint(), signal.fingerprint());
	assert!(migrated_legacy_lookup.is_none());
}

#[test]
fn autonomy_signal_store_rejects_corrupted_legacy_docs_skill_drift_row() {
	let tempdir = tempfile::tempdir().expect("tempdir should create");
	let db_path = tempdir.path().join("runtime.sqlite3");
	let signal = {
		let store = StateStore::open(&db_path).expect("store should open");

		tests::accept_objective(&store, 1);

		let signal = AutonomySignal::docs_plugin_drift(tests::signal_input())
			.expect("docs plugin signal should validate");

		store.record_autonomy_signal("decodex", signal.clone()).expect("signal should store");

		signal
	};
	let (legacy_id, _legacy_fingerprint, legacy_payload) = legacy_docs_skill_drift_payload(&signal);
	let connection = Connection::open(&db_path).expect("db should open");

	connection
		.execute(
			"UPDATE autonomy_signals
			 SET signal_id = ?3,
			     kind = 'docs_skill_drift',
			     fingerprint = 'bad-fingerprint',
			     payload_json = ?4
			 WHERE project_id = ?1 AND signal_id = ?2",
			rusqlite::params!["decodex", signal.id(), legacy_id, legacy_payload],
		)
		.expect("corrupted legacy row should update");

	let error = match StateStore::open(&db_path) {
		Ok(_) => panic!("corrupted legacy row should fail"),
		Err(error) => error,
	};

	assert!(
		error.to_string().contains("fingerprint did not match payload"),
		"unexpected error: {error}"
	);
}

fn legacy_docs_skill_drift_payload(signal: &AutonomySignal) -> (String, String, String) {
	let (legacy_id, legacy_fingerprint) =
		signal.legacy_docs_skill_drift_identity().expect("legacy identity should compute");
	let mut payload =
		serde_json::to_value(signal).expect("signal should serialize to legacy payload");

	payload["id"] = Value::String(legacy_id.clone());
	payload["fingerprint"] = Value::String(legacy_fingerprint.clone());
	payload["kind"] = Value::String(String::from("docs_skill_drift"));

	(
		legacy_id,
		legacy_fingerprint,
		serde_json::to_string(&payload).expect("legacy payload should serialize"),
	)
}
