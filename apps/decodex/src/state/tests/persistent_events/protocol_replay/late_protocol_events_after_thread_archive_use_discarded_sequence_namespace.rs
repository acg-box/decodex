use rusqlite::{Connection, Result};
use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn late_protocol_events_after_thread_archive_use_discarded_sequence_namespace() {
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
	store
		.append_event("run-1", 32, "item/started", r#"{"itemId":"item-1"}"#)
		.expect("late item start should be discarded without conflict");
	store
		.append_event("run-1", 33, "item/completed", r#"{"itemId":"item-1"}"#)
		.expect("later item completion should also be discarded");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].event_count(), 3);
	assert_eq!(runs[0].last_event_type(), Some("thread/archive"));
	assert_eq!(store.event_count("run-1").expect("event count should load"), 3);

	let connection = Connection::open(&state_path).expect("sqlite should open");
	let mut statement = connection
		.prepare(
			"SELECT sequence_number, event_type FROM protocol_events \
			 WHERE run_id = 'run-1' ORDER BY sequence_number",
		)
		.expect("protocol rows should prepare");
	let rows = statement
		.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
		.expect("protocol rows should query")
		.collect::<Result<Vec<_>>>()
		.expect("protocol rows should collect");
	let discarded_rows = rows
		.iter()
		.filter(|(_sequence, event_type)| {
			event_type.as_str() == "protocol/post_archive_event/discarded"
		})
		.collect::<Vec<_>>();

	assert!(rows.iter().any(|row| row == &(32, String::from("thread/archive"))));
	assert_eq!(discarded_rows.len(), 2);
	assert!(discarded_rows.iter().all(|(sequence, _event_type)| *sequence < 0));
	assert!(!rows.iter().any(|(_sequence, event_type)| event_type == "item/started"));
	assert!(!rows.iter().any(|(_sequence, event_type)| event_type == "item/completed"));
}
