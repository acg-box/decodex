use tempfile::TempDir;

use crate::state::{StateStore, tests::IN_PROGRESS_STATE};

#[test]
fn sqlite_lists_project_attempts_and_protocol_event_presence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let observer = StateStore::open(&state_path).expect("observer state store should open");

	writer
		.try_acquire_lease("decodex", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record project ownership");
	writer
		.record_run_attempt("run-1", "issue-1", 1, "succeeded")
		.expect("first run attempt should record");
	writer.update_run_thread("run-1", "thread-1").expect("first thread should attach");
	writer.append_event("run-1", 1, "thread/archive", "{}").expect("archive event should record");
	writer
		.try_acquire_lease("other", "issue-2", "run-2", IN_PROGRESS_STATE)
		.expect("other lease should record project ownership");
	writer
		.record_run_attempt("run-2", "issue-2", 1, "succeeded")
		.expect("other run attempt should record");

	let attempts = observer
		.list_run_attempts_for_project("decodex")
		.expect("project attempts should load from sqlite");

	assert_eq!(attempts.len(), 1);
	assert_eq!(attempts[0].run_id(), "run-1");
	assert_eq!(attempts[0].thread_id(), Some("thread-1"));
	assert!(
		observer
			.run_has_protocol_event("run-1", "thread/archive")
			.expect("sqlite event presence should load")
	);
	assert!(
		!observer
			.run_has_protocol_event("run-2", "thread/archive")
			.expect("sqlite missing event presence should load")
	);
}
