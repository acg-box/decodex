use crate::state::StateStore;

#[test]
fn records_run_attempts_and_events() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should be recorded");
	store.update_run_thread("run-1", "thread-1").expect("thread id should be attached");
	store
		.append_event("run-1", 1, "turn/started", "{\"turn\":\"1\"}")
		.expect("event should be recorded");

	let run_attempt = store
		.run_attempt("run-1")
		.expect("run attempt query should succeed")
		.expect("run attempt should exist");

	assert_eq!(run_attempt.issue_id(), "PUB-101");
	assert_eq!(run_attempt.attempt_number(), 1);
	assert_eq!(run_attempt.status(), "running");
	assert_eq!(run_attempt.thread_id(), Some("thread-1"));
	assert_eq!(store.event_count("run-1").expect("event count should succeed"), 1);
	assert_eq!(store.next_attempt_number("PUB-101").expect("next attempt should load"), 2);
	assert_eq!(
		store.retry_budget_attempt_count("PUB-101").expect("retry budget count should load"),
		0
	);

	store.update_run_status("run-1", "interrupted").expect("status should update");

	let updated = store
		.run_attempt("run-1")
		.expect("run attempt query should succeed")
		.expect("run attempt should exist");

	assert_eq!(updated.status(), "interrupted");
	assert!(
		store
			.last_run_activity_unix_epoch("run-1")
			.expect("last activity lookup should succeed")
			.is_some()
	);
}
