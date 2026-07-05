use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn post_archive_discard_append_rebuilds_stale_compacted_protocol_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "failed").expect("run attempt should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record");
	store
		.append_event("run-1", 32, "thread/archive", r#"{"threadId":"thread-1"}"#)
		.expect("archive event should append");

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"UPDATE protocol_event_summaries
			 SET event_count = 1,
			     last_sequence_number = 32,
			     last_event_type = 'thread/archive'
			 WHERE run_id = 'run-1'",
			[],
		)
		.expect("compacted summary should be pinned stale");
	store
		.append_event("run-1", 32, "item/started", r#"{"itemId":"item-1"}"#)
		.expect("late item start should be discarded without conflict");

	assert_eq!(store.event_count("run-1").expect("event count should load"), 2);

	let compacted_event_count: i64 = connection
		.query_row(
			"SELECT event_count FROM protocol_event_summaries WHERE run_id = 'run-1'",
			[],
			|row| row.get(0),
		)
		.expect("compacted summary should load");

	assert_eq!(compacted_event_count, 2);
}
