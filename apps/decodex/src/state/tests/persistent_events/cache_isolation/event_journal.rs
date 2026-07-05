use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn persistent_append_event_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&state_path).expect("first state store should open");
	let second = StateStore::open(&state_path).expect("second state store should open");

	first.record_run_attempt("run-a", "PUB-101", 1, "running").expect("first run should record");
	second.record_run_attempt("run-b", "PUB-102", 1, "running").expect("second run should record");
	second
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("second store should append an unrelated event");
	first
		.append_event("run-a", 1, "item/agentMessage/delta", "{}")
		.expect("first store should append without full journal refresh");

	let state = first.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"append_event should not refresh the full persistent event journal into the local cache"
	);

	drop(state);

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(reopened.event_count("run-a").expect("first event count should load"), 1);
	assert_eq!(reopened.event_count("run-b").expect("second event count should load"), 1);
}

#[test]
fn persistent_run_attempt_update_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&state_path).expect("first state store should open");
	let second = StateStore::open(&state_path).expect("second state store should open");

	second.record_run_attempt("run-b", "PUB-102", 1, "running").expect("second run should record");
	second
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("second store should append an unrelated event");
	first.record_run_attempt("run-a", "PUB-101", 1, "running").expect("first run should record");
	first.update_run_thread("run-a", "thread-a").expect("first run thread should update");
	first.update_run_turn("run-a", "turn-a").expect("first run turn should update");
	first.update_run_status("run-a", "succeeded").expect("first run status should update");

	let state = first.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"run attempt updates should not refresh the full persistent event journal into the local cache"
	);

	drop(state);

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");
	let attempt = reopened
		.run_attempt("run-a")
		.expect("run attempt lookup should succeed")
		.expect("run attempt should persist");

	assert_eq!(attempt.status(), "succeeded");
	assert_eq!(attempt.thread_id(), Some("thread-a"));
	assert_eq!(attempt.turn_id(), Some("turn-a"));
	assert_eq!(reopened.event_count("run-b").expect("second event count should load"), 1);
}
