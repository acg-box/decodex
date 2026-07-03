use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, RUN_ACTIVITY_MARKER_FILE, StateStore, TEST_SERVICE_ID, fs, orchestrator,
	tracker,
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

#[test]
fn live_operator_status_snapshot_inspects_unreadable_active_label_worktree() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-unreadable-active",
		"PUB-113",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::write(worktree_path.join(".git"), "gitdir: /does/not/exist\n")
		.expect("invalid gitdir should write");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-113",
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
		.find(|candidate| candidate.issue_identifier == "PUB-113")
		.expect("unreadable active-label issue should remain visible");
	let attention = candidate.attention.as_ref().expect("recovery details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "linear_active_label_present");
	assert!(!attention.worktree_has_tracked_changes);
	assert_eq!(
		attention.attention_next_action.as_deref(),
		Some("inspect_retained_worktree_changes_before_stale_active_recovery")
	);
	assert!(
		attention.summary.contains("worktree cleanliness could not be verified"),
		"summary should explain unreadable retained worktree, got {:?}",
		attention.summary
	);
}

#[test]
fn live_operator_status_snapshot_inspects_unreadable_active_label_marker() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-unreadable-marker-active",
		"PUB-116",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	fs::create_dir_all(worktree_path.join(RUN_ACTIVITY_MARKER_FILE))
		.expect("directory marker should create");

	state_store
		.record_run_attempt("run-116", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-116",
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
		.find(|candidate| candidate.issue_identifier == "PUB-116")
		.expect("unreadable marker active-label issue should remain visible");
	let attention = candidate.attention.as_ref().expect("recovery details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "linear_active_label_present");
	assert!(!attention.worktree_has_tracked_changes);
	assert_eq!(
		attention.attention_next_action.as_deref(),
		Some("inspect_retained_worktree_changes_before_stale_active_recovery")
	);
	assert!(
		attention.summary.contains("worktree cleanliness could not be verified"),
		"summary should explain unreadable marker, got {:?}",
		attention.summary
	);
}

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
