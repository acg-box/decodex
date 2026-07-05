use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn late_protocol_events_after_archive_discard_are_restart_safe() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "failed").expect("run attempt should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record");
	store
		.append_event("run-1", 7, "thread/archive/discarded", r#"{"threadId":"thread-1"}"#)
		.expect("discarded archive event should append");

	drop(store);

	let restarted = StateStore::open_lazy(&state_path).expect("lazy store should open");

	restarted
		.append_event("run-1", 8, "item/commandExecution/outputDelta", r#"{"delta":"late output"}"#)
		.expect("late output after restart should be discarded without conflict");

	let runs = restarted.list_recent_runs("pubfi", 10).expect("recent runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].event_count(), 2);
	assert_eq!(runs[0].last_event_type(), Some("thread/archive/discarded"));
	assert_eq!(restarted.event_count("run-1").expect("event count should load"), 2);
}
