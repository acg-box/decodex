use rusqlite::{Connection, Result};
use tempfile::TempDir;

use crate::state::{StateStore, tests::IN_PROGRESS_STATE};

#[test]
fn protocol_event_replay_is_idempotent_when_payload_matches() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let payload = r#"{"threadId":"thread-1","attemptNumber":4}"#;

	store
		.append_event("run-1", 32, "thread/archive", payload)
		.expect("first archive event should append");
	store
		.append_event("run-1", 32, "thread/archive", payload)
		.expect("matching archive replay should be idempotent");

	assert_eq!(store.event_count("run-1").expect("event count should load"), 1);

	let connection = Connection::open(&state_path).expect("sqlite should open");
	let payload_sha256: String = connection
		.query_row(
			"SELECT payload_sha256 FROM protocol_events WHERE run_id = 'run-1' AND sequence_number = 32",
			[],
			|row| row.get(0),
		)
		.expect("payload digest should read");

	assert_eq!(payload_sha256.len(), 64);
	assert!(payload_sha256.chars().all(|character| character.is_ascii_hexdigit()));
}

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

#[test]
fn protocol_event_sequence_conflict_rejects_changed_payload() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.append_event("run-1", 32, "thread/archive", r#"{"threadId":"thread-1"}"#)
		.expect("first archive event should append");

	let error = store
		.append_event("run-1", 32, "thread/archive", r#"{"threadId":"thread-2"}"#)
		.expect_err("changed payload at the same sequence should be rejected");

	assert!(
		error.to_string().contains("conflicts with an existing runtime journal event"),
		"unexpected error: {error:?}",
	);
	assert_eq!(store.event_count("run-1").expect("event count should load"), 1);
}

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
