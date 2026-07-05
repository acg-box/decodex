use std::fs;

use tempfile::TempDir;

use crate::{
	recovery::{
		self, RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::{self, StateStore},
	tracker,
};

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_private_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");

	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.append_private_execution_event(
			"pubfi",
			&issue.identifier,
			"run-identifier",
			1,
			"source_progress",
			serde_json::json!({"phase": "implementation"}),
		)
		.expect("identifier-keyed private progress should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = recovery::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, recovery::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_worktree_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let worktree_path = temp_dir.path().join("identifier-worktree");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");

	fs::create_dir_all(&worktree_path).expect("identifier worktree should create");
	fs::write(worktree_path.join("source.rs"), "fn progress() {}\n")
		.expect("ordinary worktree file should write");

	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.identifier,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("identifier-keyed worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = recovery::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.worktree_state, "non_git_files_present");
	assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_active_thread_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	fs::create_dir_all(&worktree_path).expect("worktree path should create");
	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1626",
		1,
		Some("thread-1626"),
		Some("turn-1626"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("active thread marker should write");

	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = recovery::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, recovery::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("activity_marker_thread_active")));
	assert!(!diagnostic.recoverable());
}
