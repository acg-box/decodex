use tempfile::TempDir;

use crate::{
	state::{StateStore, tests::IN_PROGRESS_STATE},
	tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

#[test]
fn persistent_retry_budget_queries_do_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");

	writer
		.record_run_attempt("run-a", "PUB-101", 1, "interrupted")
		.expect("writer retry attempt should record");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");

	assert_eq!(observer.retry_budget_attempt_count("PUB-101").expect("retry count should read"), 1);
	assert!(
		observer
			.issue_has_retry_budget_attempt_after("PUB-101", 0)
			.expect("retry after query should read")
	);
	assert!(
		!observer
			.issue_has_retry_budget_attempt_after("PUB-101", 1)
			.expect("retry after query should read")
	);

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"retry-budget queries should not refresh the full persistent event journal into the local cache"
	);
	assert!(
		!state.event_summaries.contains_key("run-b"),
		"retry-budget queries should not refresh protocol summaries unrelated to the issue"
	);
	assert!(
		!state.run_attempts.contains_key("run-a"),
		"retry-budget queries should use issue-scoped persistent reads instead of a full runtime refresh"
	);
}

#[test]
fn persistent_shared_claim_check_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let holder = StateStore::open(&state_path).expect("holder state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let slot_root = temp_dir.path().join("slots");

	observer
		.configure_dispatch_slot_root("pubfi", &slot_root)
		.expect("observer slot root should configure");
	holder
		.configure_dispatch_slot_root("pubfi", &slot_root)
		.expect("holder slot root should configure");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");

	assert!(
		holder
			.try_acquire_lease("pubfi", "PUB-101", "run-a", IN_PROGRESS_STATE)
			.expect("holder should acquire the shared issue claim")
	);
	assert!(
		observer
			.issue_has_active_shared_claim("pubfi", "PUB-101")
			.expect("shared claim check should read")
	);

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"shared claim checks should not refresh the full persistent event journal into the local cache"
	);
	assert!(
		!state.event_summaries.contains_key("run-b"),
		"shared claim checks should not refresh protocol summaries unrelated to the issue"
	);
}

#[test]
fn persistent_linear_execution_event_listing_does_not_refresh_full_ledger() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let mut writer_record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-102",
			issue_identifier: "PUB-102",
			run_id: "run-b",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-04-29T10:12:00Z"),
		"closeout",
	);

	writer_record.summary = Some(String::from("Writer closeout."));
	writer_record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/102"));
	writer_record.commit_sha = Some(String::from("2222222222222222222222222222222222222222"));

	writer
		.record_linear_execution_event(&writer_record)
		.expect("writer ledger event should persist");

	let observed = observer
		.list_linear_execution_events("pubfi", "PUB-102")
		.expect("observer should read issue-scoped ledger events");

	assert_eq!(observed, vec![writer_record.clone()]);

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.linear_execution_events.contains_key(&writer_record.idempotency_key),
		"issue-scoped ledger listing should not refresh the full persistent ledger into the local cache"
	);
}
