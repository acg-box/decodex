use tempfile::TempDir;

use crate::state::{StateStore, tests::IN_PROGRESS_STATE};

#[test]
fn sparse_protocol_event_replay_keeps_leased_run_summary_count_exact() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let payload = r#"{"threadId":"thread-1","attemptNumber":4}"#;

	store.record_run_attempt("run-1", "PUB-101", 1, "running").expect("run attempt should persist");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should persist");
	store
		.append_event("run-1", 32, "thread/archive", payload)
		.expect("first sparse event should append");
	store
		.append_event("run-1", 32, "thread/archive", payload)
		.expect("matching sparse replay should be idempotent");

	let runs = store.list_leased_runs("pubfi").expect("leased runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert_eq!(runs[0].event_count(), 1);
	assert_eq!(runs[0].last_event_type(), Some("thread/archive"));
}
