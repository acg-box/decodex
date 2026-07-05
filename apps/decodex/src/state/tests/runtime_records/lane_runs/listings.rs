use std::path::Path;

use crate::state::{StateStore, tests::runtime_records::IN_PROGRESS_STATE};

#[test]
fn lists_issue_leases() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("first lease should be inserted");
	store
		.upsert_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
		.expect("second lease should be inserted");

	let leases = store.list_leases("pubfi").expect("lease listing should succeed");

	assert_eq!(leases.len(), 2);
	assert_eq!(leases[0].project_id(), "pubfi");
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(leases[1].issue_id(), "PUB-102");
}

#[test]
fn lists_recent_project_runs_with_protocol_summary() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-2", "PUB-102", 2, "failed")
		.expect("older run attempt should be recorded");
	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("running run attempt should be recorded");
	store.update_run_thread("run-1", "thread-1").expect("thread id should attach");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("active worktree should record");
	store
		.upsert_worktree("pubfi", "PUB-102", "x/pubfi-pub-102", "/tmp/worktrees/pub-102")
		.expect("retained worktree should record");
	store
		.append_event("run-1", 1, "turn/started", "{\"turn\":\"1\"}")
		.expect("event should record");
	store
		.append_event("run-1", 2, "turn/completed", "{\"turn\":\"1\"}")
		.expect("second event should record");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 2);
	assert_eq!(runs[0].run_id(), "run-1");
	assert!(runs[0].run_lease());
	assert_eq!(runs[0].thread_id(), Some("thread-1"));
	assert_eq!(runs[0].event_count(), 2);
	assert_eq!(runs[0].last_event_type(), Some("turn/completed"));
	assert_eq!(runs[0].branch_name(), Some("x/pubfi-pub-101"));
	assert_eq!(runs[0].worktree_path(), Some(Path::new("/tmp/worktrees/pub-101")));
	assert_eq!(runs[1].run_id(), "run-2");
	assert!(!runs[1].run_lease());
	assert_eq!(runs[1].event_count(), 0);
}

#[test]
fn lists_recent_project_runs_after_terminal_lane_cleanup() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should record before project ownership is known");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record project ownership");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record project ownership");
	store.update_run_status("run-1", "succeeded").expect("terminal status should update");
	store.clear_lease("PUB-101").expect("terminal cleanup should clear run lease");
	store.clear_worktree("PUB-101").expect("terminal cleanup should clear worktree mapping");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert_eq!(runs[0].status(), "succeeded");
	assert!(!runs[0].run_lease());
	assert_eq!(runs[0].branch_name(), None);
	assert_eq!(runs[0].worktree_path(), None);
	assert!(
		store.list_recent_runs("other", 10).expect("other project lookup should load").is_empty(),
		"remembered run ownership must stay scoped to the original project"
	);
}

#[test]
fn lists_active_project_runs_only() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "running").expect("first run should record");
	store.record_run_attempt("run-2", "PUB-102", 1, "running").expect("second run should record");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.upsert_lease("other", "PUB-102", "run-2", IN_PROGRESS_STATE)
		.expect("other-project lease should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("first worktree should record");
	store
		.upsert_worktree("other", "PUB-102", "x/other-pub-102", "/tmp/worktrees/pub-102")
		.expect("second worktree should record");

	let runs = store.list_leased_runs("pubfi").expect("active project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert!(runs[0].run_lease());
}
