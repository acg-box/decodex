use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, fs, orchestrator, tracker,
};

#[test]
fn live_operator_status_snapshot_inspects_non_git_active_label_files() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-non-git-active",
		"PUB-115",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join("mapped-retained-PUB-115");
	let tracker = FakeTracker::new(vec![issue.clone()]);

	fs::create_dir_all(&worktree_path).expect("retained path should exist");
	fs::write(worktree_path.join("retained.txt"), "retained work\n")
		.expect("retained file should write");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-115",
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
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == "PUB-115")
		.expect("non-git active-label issue should remain visible");
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
		"summary should explain non-git retained files, got {:?}",
		attention.summary
	);
}
