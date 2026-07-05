use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::StateStore;

#[test]
fn manages_worktree_mappings() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_worktree("pubfi", "PUB-101", "x/pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");

	let mapping = store
		.worktree_for_issue("PUB-101")
		.expect("mapping lookup should succeed")
		.expect("mapping should exist");

	assert_eq!(mapping.issue_id(), "PUB-101");
	assert_eq!(mapping.branch_name(), "x/pub-101");
	assert_eq!(mapping.worktree_path(), Path::new("/tmp/worktrees/pub-101"));
	assert_eq!(mapping.project_id(), "pubfi");
	assert_eq!(mapping.provenance().source(), "runtime_recorded");
	assert!(mapping.provenance().created_at_unix().is_some());
	assert!(mapping.provenance().updated_at_unix().is_some());
	assert_eq!(store.list_worktrees("pubfi").expect("list should succeed").len(), 1);

	store.clear_worktree("PUB-101").expect("mapping should be deleted");

	assert!(store.worktree_for_issue("PUB-101").expect("lookup should succeed").is_none());
}

#[test]
fn opens_legacy_worktree_rows_with_unknown_provenance() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let db_path = temp_dir.path().join("runtime.sqlite3");

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('issue-legacy', 'pubfi', 'x/pubfi-pub-101', '/tmp/worktrees/pub-101');",
			)
			.expect("legacy worktree row should write");
	}

	let store = StateStore::open(&db_path).expect("state store should migrate");
	let mapping = store
		.worktree_for_issue("issue-legacy")
		.expect("mapping lookup should succeed")
		.expect("legacy mapping should exist");

	assert_eq!(mapping.provenance().source(), "legacy_unknown");
	assert_eq!(mapping.provenance().created_at_unix(), None);
	assert_eq!(mapping.provenance().updated_at_unix(), None);
}
