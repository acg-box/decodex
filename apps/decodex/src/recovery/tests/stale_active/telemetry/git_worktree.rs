use tempfile::TempDir;

use crate::{
	recovery::{
		RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::StateStore,
	tracker,
};

#[test]
fn stale_active_diagnose_blocks_clean_worktree_with_unmerged_commits() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	tests::init_git_repo(&worktree_path);
	tests::run_git(&worktree_path, &["checkout", "-B", "main"]);
	tests::commit_test_file(&worktree_path, "README.md", "base\n", "base");
	tests::run_git(&worktree_path, &["checkout", "-b", "x/pubfi-pub-1626"]);
	tests::commit_test_file(&worktree_path, "source.rs", "fn retained_progress() {}\n", "progress");

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
	let diagnostics = super::diagnose_stale_active_issues(
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

	assert_eq!(diagnostic.worktree_state, "unmerged_commits_present");
	assert!(diagnostic.blockers.contains(&String::from("worktree_unmerged_commits_present")));
	assert!(
		diagnostic.next_action.contains("Preserve retained progress")
			&& diagnostic.next_action.contains("inspect the retained worktree"),
		"retained worktree blockers should route to retained-progress inspection, got {:?}",
		diagnostic.next_action
	);
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_clean_git_worktree_without_default_branch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = tests::sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	tests::init_git_repo(&worktree_path);

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
	let diagnostics = super::diagnose_stale_active_issues(
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

	assert_eq!(diagnostic.worktree_state, "default_branch_unavailable");
	assert!(diagnostic.blockers.contains(&String::from("worktree_default_branch_unavailable")));
	assert!(!diagnostic.recoverable());
}
