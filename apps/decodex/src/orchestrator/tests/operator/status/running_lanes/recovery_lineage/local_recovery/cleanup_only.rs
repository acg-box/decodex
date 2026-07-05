use crate::orchestrator::tests::operator::status::running_lanes::{
	self, Connection, FakeTracker, StateStore, TempDir, fs, orchestrator,
};

#[test]
fn legacy_cleanup_only_worktree_requires_audited_manual_closeout() {
	let (temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let db_path = temp_dir.path().join("legacy-runtime.sqlite3");
	let issue = running_lanes::sample_issue_with_sort_fields(
		"issue-cleanup",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(&format!(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('{}', 'pubfi', 'x/pubfi-pub-199', '{}');",
				issue.id,
				worktree_path.display()
			))
			.expect("legacy worktree row should write");
	}

	let tracker = FakeTracker::new(vec![issue]);
	let state_store = StateStore::open(&db_path).expect("state store should migrate");
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert_eq!(snapshot.worktrees[0].provenance.source, "legacy_unknown");
	assert!(snapshot.worktrees[0].provenance.audit_required);
	assert!(
		snapshot.worktrees[0]
			.recovery_next_action
			.as_deref()
			.is_some_and(|action| action.contains("decodex recover legacy-closeout PUB-199"))
	);
	assert_eq!(snapshot_json["worktrees"][0]["provenance"]["source"], "legacy_unknown");
	assert_eq!(snapshot_json["worktrees"][0]["provenance"]["audit_required"], true);
	assert!(rendered.contains("provenance_source: legacy_unknown"));
	assert!(rendered.contains("audit_required: true"));
	assert!(rendered.contains("recovery_next_action: verify tracker/PR terminal state"));
	assert!(rendered.contains("decodex recover legacy-closeout PUB-199"));
}

#[test]
fn runtime_recovery_preserves_legacy_cleanup_only_provenance_without_recoverable_owner() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let db_path = temp_dir.path().join("runtime.sqlite3");
	let (_layout_dir, config, workflow) = running_lanes::temp_project_layout();
	let issue = running_lanes::sample_issue_with_sort_fields(
		"issue-legacy",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("legacy worktree path should exist");

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(&format!(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('{}', 'pubfi', 'x/pubfi-pub-199', '{}');",
				issue.id,
				worktree_path.display()
			))
			.expect("legacy worktree row should write");
	}

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open(&db_path).expect("state store should migrate");
	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");
	let mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("legacy mapping should remain");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"terminal cleanup-only worktree should not become a retry lane"
	);
	assert_eq!(mapping.provenance().source(), "legacy_unknown");
	assert_eq!(mapping.provenance().created_at_unix(), None);
	assert_eq!(mapping.provenance().updated_at_unix(), None);
}
