use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn pre_archive_protocol_replays_remain_idempotent_after_archive() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "failed").expect("run attempt should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record");
	store
		.append_event("run-1", 31, "item/started", r#"{"itemId":"item-1"}"#)
		.expect("pre-archive item start should append");
	store
		.append_event("run-1", 32, "thread/archive", r#"{"threadId":"thread-1"}"#)
		.expect("archive event should append");
	store
		.append_event("run-1", 31, "item/started", r#"{"itemId":"item-1"}"#)
		.expect("pre-archive replay should stay idempotent after archive");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].event_count(), 2);
	assert_eq!(runs[0].last_event_type(), Some("thread/archive"));
	assert_eq!(store.event_count("run-1").expect("event count should load"), 2);
}
