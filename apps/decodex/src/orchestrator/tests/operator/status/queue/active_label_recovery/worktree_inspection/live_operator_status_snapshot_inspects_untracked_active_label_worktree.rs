use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, fs, orchestrator, tracker,
};

#[test]
fn live_operator_status_snapshot_inspects_untracked_active_label_worktree() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-untracked-active",
		"PUB-114",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	status::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-114", ".worktrees/PUB-114", "main"],
	);
	fs::write(worktree_path.join("new_source.rs"), "fn retained_progress() {}\n")
		.expect("untracked source file should write");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == "PUB-114")
		.expect("untracked active-label issue should remain visible");
	let attention = candidate.attention.as_ref().expect("recovery details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "linear_active_label_present");
	assert!(attention.worktree_has_tracked_changes);
	assert_eq!(
		attention.attention_next_action.as_deref(),
		Some("inspect_retained_worktree_changes_before_stale_active_recovery")
	);
	assert!(
		attention.summary.contains("retained worktree changes"),
		"summary should explain untracked retained worktree, got {:?}",
		attention.summary
	);
}
