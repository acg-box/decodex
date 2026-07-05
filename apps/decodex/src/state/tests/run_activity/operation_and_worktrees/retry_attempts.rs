use crate::state::StateStore;

#[test]
fn counts_retry_budget_attempts_per_issue() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "succeeded").expect("first run should record");
	store.record_run_attempt("run-2", "PUB-101", 2, "failed").expect("second run should record");
	store
		.record_run_attempt("run-3", "PUB-101", 3, "interrupted")
		.expect("third run should record");
	store
		.record_run_attempt("run-5", "PUB-101", 4, "terminal_guarded")
		.expect("guarded run should record");
	store
		.record_run_attempt("run-4", "PUB-102", 1, "failed")
		.expect("other issue run should record");

	assert_eq!(
		store.retry_budget_attempt_count("PUB-101").expect("retry budget count should load"),
		3
	);
	assert_eq!(
		store.retry_budget_attempt_count("PUB-102").expect("retry budget count should load"),
		1
	);
}

#[test]
fn loads_latest_run_attempt_for_issue() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "failed").expect("first run should record");
	store
		.record_run_attempt("run-2", "PUB-101", 2, "terminal_guarded")
		.expect("latest run should record");

	let attempt = store
		.latest_run_attempt_for_issue("PUB-101")
		.expect("latest run lookup should succeed")
		.expect("latest run should exist");

	assert_eq!(attempt.run_id(), "run-2");
	assert_eq!(attempt.attempt_number(), 2);
	assert_eq!(attempt.status(), "terminal_guarded");
}
