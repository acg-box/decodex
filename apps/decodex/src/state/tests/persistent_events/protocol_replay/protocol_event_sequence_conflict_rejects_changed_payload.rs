use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn protocol_event_sequence_conflict_rejects_changed_payload() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.append_event("run-1", 32, "thread/archive", r#"{"threadId":"thread-1"}"#)
		.expect("first archive event should append");

	let error = store
		.append_event("run-1", 32, "thread/archive", r#"{"threadId":"thread-2"}"#)
		.expect_err("changed payload at the same sequence should be rejected");

	assert!(
		error.to_string().contains("conflicts with an existing runtime journal event"),
		"unexpected error: {error:?}",
	);
	assert_eq!(store.event_count("run-1").expect("event count should load"), 1);
}
