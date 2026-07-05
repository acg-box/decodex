use std::fs;

use tempfile::TempDir;

use crate::{
	recovery::{
		self, RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::{ChildAgentActivitySummary, StateStore},
	tracker::{self},
};

#[test]
fn stale_active_diagnose_blocks_untracked_worktree_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	tests::init_git_repo(&worktree_path);
	fs::write(worktree_path.join("new_source.rs"), "fn retained_progress() {}\n")
		.expect("untracked source should write");

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

	assert_eq!(diagnostic.classification, crate::recovery::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_non_git_retained_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	fs::create_dir_all(&worktree_path).expect("retained path should create");
	fs::write(worktree_path.join("retained.txt"), "retained work\n")
		.expect("retained file should write");

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

	assert_eq!(diagnostic.worktree_state, "non_git_files_present");
	assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_child_agent_activity_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let activity = ChildAgentActivitySummary { event_count: 1, ..Default::default() };
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

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
		.record_run_activity_summary("run-1626", 1, Some(&activity), None)
		.expect("child activity should record");

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

	assert_eq!(diagnostic.classification, crate::recovery::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
	assert!(!diagnostic.recoverable());
}
