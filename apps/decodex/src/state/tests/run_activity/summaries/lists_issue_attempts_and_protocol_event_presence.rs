use crate::state::StateStore;

#[test]
fn lists_issue_attempts_and_protocol_event_presence() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-2", "PUB-101", 2, "succeeded")
		.expect("second run attempt should record");
	store
		.record_run_attempt("run-1", "PUB-101", 1, "failed")
		.expect("first run attempt should record");
	store
		.record_run_attempt("run-other", "PUB-102", 1, "succeeded")
		.expect("other issue run attempt should record");
	store.update_run_thread("run-1", "thread-1").expect("first thread should attach");
	store.update_run_thread("run-2", "thread-2").expect("second thread should attach");
	store.append_event("run-1", 1, "thread/archive", "{}").expect("archive event should record");

	let attempts =
		store.list_run_attempts_for_issue("PUB-101").expect("issue attempts should load");

	assert_eq!(attempts.len(), 2);
	assert_eq!(attempts[0].run_id(), "run-1");
	assert_eq!(attempts[0].thread_id(), Some("thread-1"));
	assert_eq!(attempts[1].run_id(), "run-2");
	assert!(store.run_has_protocol_event("run-1", "thread/archive").expect("event should load"));
	assert!(
		!store
			.run_has_protocol_event("run-2", "thread/archive")
			.expect("missing event should load")
	);
}
