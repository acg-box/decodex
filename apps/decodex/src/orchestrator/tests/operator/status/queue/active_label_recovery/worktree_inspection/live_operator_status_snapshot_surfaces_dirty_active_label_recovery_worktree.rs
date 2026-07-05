use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, fs, orchestrator, tracker,
};

#[test]
fn live_operator_status_snapshot_surfaces_dirty_active_label_recovery_worktree() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-dirty-active",
		"PUB-112",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	status::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-112", ".worktrees/PUB-112", "main"],
	);
	fs::write(worktree_path.join("README.md"), "dirty active-label patch\n")
		.expect("tracked worktree file should change");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == "PUB-112")
		.expect("dirty active-label issue should remain visible");
	let attention = candidate.attention.as_ref().expect("recovery details should render");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.issue_identifier.as_deref() == Some("PUB-112"))
		.expect("retained worktree should remain visible");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "linear_active_label_present");
	assert_eq!(attention.attention_error_class.as_deref(), Some("evidence_missing"));
	assert!(attention.worktree_has_tracked_changes);
	assert_eq!(
		attention.attention_next_action.as_deref(),
		Some("inspect_retained_worktree_changes_before_stale_active_recovery")
	);
	assert!(
		attention.summary.contains("retained worktree changes"),
		"summary should explain dirty retained recovery, got {:?}",
		attention.summary
	);
	assert_eq!(worktree.ownership, "queued_attention");
	assert_eq!(project.attention_count, 1);
	assert_eq!(project.retained_worktree_count, 0);
}
