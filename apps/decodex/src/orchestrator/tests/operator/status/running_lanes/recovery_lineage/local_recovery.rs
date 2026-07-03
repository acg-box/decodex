use crate::orchestrator::tests::{
	operator::status::{
		running_lanes,
		running_lanes::{Connection, FakeTracker, StateStore, TempDir, fs, orchestrator, state},
	},
	recovery_terminal_support,
};
#[test]
fn operator_status_snapshot_includes_local_recovery_worktree_directories() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-199");

	fs::create_dir_all(&worktree_path).expect("worktree directory should exist");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert_eq!(snapshot.worktrees.len(), 1);
	assert_eq!(snapshot.worktrees[0].issue_id, "PUB-199");
	assert!(!snapshot.worktrees[0].branch_name.is_empty());
	assert_eq!(snapshot.worktrees[0].worktree_path, ".worktrees/PUB-199");
	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert!(snapshot.worktrees[0].ownership_reason.contains("local cleanup only"));
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
}

#[test]
fn completed_retained_worktree_without_post_review_owner_is_cleanup_only() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue_with_sort_fields(
		"issue-cleanup",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-199",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

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

	assert!(snapshot.post_review_lanes.is_empty());
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].issue_identifier.as_deref(), Some("PUB-199"));
	assert_eq!(snapshot.worktrees[0].issue_state.as_deref(), Some("Done"));
	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert!(snapshot.worktrees[0].ownership_reason.contains("Issue is Done"));
	assert_eq!(snapshot_json["worktrees"][0]["ownership"], "cleanup_only");
	assert_eq!(snapshot_json["worktrees"][0]["issue_state"], "Done");
	assert!(rendered.contains("role: cleanup_only"));
	assert!(rendered.contains("reason: Issue is Done"));
	assert!(!rendered.contains("role: post_review_lane"));
	assert!(!rendered.contains("classification: blocked"));
	assert!(!rendered.contains("review_handoff_missing"));
}

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

#[test]
fn runtime_recovery_records_recovered_provenance_for_fresh_active_worktree() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("active worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-1", 1)
		.expect("activity marker should write");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("activity marker should load")
		.expect("activity marker should exist");
	let observed_at_unix =
		marker.last_activity_unix_epoch().expect("activity marker should have a stable timestamp");
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
		.expect("recovered mapping should exist");
	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("fresh active marker should recover the lease");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"fresh marker should recover as the run lease instead of a retry queue item"
	);
	assert_eq!(mapping.provenance().source(), "runtime_recovered");
	assert_eq!(mapping.provenance().created_at_unix(), Some(observed_at_unix));
	assert_eq!(mapping.provenance().updated_at_unix(), Some(observed_at_unix));
	assert_eq!(lease.run_id(), "run-1");
}

#[test]
fn runtime_recovery_splits_invalid_local_id_batch_without_losing_valid_issue() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let mut issue = recovery_terminal_support::sample_active_issue("In Progress");

	issue.id = String::from("00000000-0000-0000-0000-000000000101");

	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("active worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-101", 1)
		.expect("activity marker should write");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("valid worktree mapping should record");
	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("invalid local run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("invalid local lease should record");

	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should split invalid local ids from valid server ids");
	let recovered_mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("valid issue mapping should remain");
	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("valid issue lease should recover");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"fresh valid issue should recover as active lease rather than disappear"
	);
	assert_eq!(recovered_mapping.issue_id(), issue.id);
	assert_eq!(lease.issue_id(), issue.id);
	assert_eq!(lease.run_id(), "run-101");
}

#[test]
fn post_review_worktree_refresh_splits_invalid_local_id_batch_without_losing_valid_issue() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let mut issue = recovery_terminal_support::sample_active_issue("In Review");

	issue.id = String::from("00000000-0000-0000-0000-000000000101");

	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let valid_worktree_path = config.worktree_root().join(&issue.identifier);
	let missing_ghost_path = config.worktree_root().join("PUB-012");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&valid_worktree_path.display().to_string(),
		)
		.expect("valid worktree mapping should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&missing_ghost_path.display().to_string(),
		)
		.expect("stale local-id worktree mapping should record");

	let worktree_issues =
		orchestrator::load_post_review_worktree_issues(&tracker, &config, &state_store)
			.expect("post-review refresh should split invalid local ids from valid server ids");
	let (worktree, refreshed_issue) =
		worktree_issues.first().expect("valid post-review worktree issue should remain");

	assert_eq!(worktree_issues.len(), 1);
	assert_eq!(worktree.issue_id(), issue.id);
	assert_eq!(refreshed_issue.id, issue.id);
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.any(|query| query == &vec![String::from("PUB-012")]),
		"stale local issue id should be retried in isolation"
	);
}
