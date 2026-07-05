use crate::state::StateStore;

#[test]
fn manual_attention_failure_overrides_succeeded_run_status() {
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-1", "issue-1", 1, "succeeded")
		.expect("run attempt should record");
	state_store.update_run_status("run-1", "failed").expect("failed outcome should persist");

	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"failed"
	);
}
