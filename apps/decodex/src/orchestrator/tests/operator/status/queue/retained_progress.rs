use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, fs, orchestrator, state,
};

#[test]
fn live_operator_status_snapshot_surfaces_retained_partial_progress() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-106",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	status::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-106", ".worktrees/PUB-106", "main"],
	);
	fs::write(worktree_path.join("README.md"), "changed repo file\n")
		.expect("tracked worktree file should change");
	state::write_run_retry_budget_attempt_count(&worktree_path, "run-partial-progress", 3, 3)
		.expect("retry budget marker should write");

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
		.find(|candidate| candidate.issue_identifier == "PUB-106")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert!(attention.worktree_has_tracked_changes);
	assert_eq!(attention.retry_budget_attempt_count, Some(3));
	assert_eq!(attention.retry_budget_max_attempts, 3);
	assert!(
		attention.summary.contains("Partial worktree changes are retained"),
		"summary should explain retained patch recovery, got {:?}",
		attention.summary
	);
}

#[test]
fn live_operator_status_snapshot_surfaces_stalled_retained_partial_progress() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-110",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	status::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-110", ".worktrees/PUB-110", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained stalled patch\n")
		.expect("tracked worktree file should change");

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
			vec![status::linear_execution_history_comment(
				&issue,
				"needs_attention",
				"2026-03-13T09:20:00Z",
				"stalled-retained-partial-progress",
				|record| {
					record.error_class = Some(String::from("partial_progress_retained"));
					record.next_action = Some(String::from(
						"inspect retained worktree `.worktrees/PUB-110`, finish validation and PR handoff or reset the patch manually",
					));
					record.terminal_path = Some(String::from("retained_partial_progress"));
					record.summary = Some(String::from(
						"Decodex retained partial progress and needs attention.",
					));
					record.blockers = Some(vec![String::from(
						"tracked worktree changes were retained after stalled reconciliation",
					)]);
				record.evidence = Some(vec![String::from(
					"worktree `.worktrees/PUB-110` has tracked changes",
				)]);
			},
		)],
	);

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
		.find(|candidate| candidate.issue_identifier == "PUB-110")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(attention.attention_error_class.as_deref(), Some("partial_progress_retained"));
	assert!(attention.worktree_has_tracked_changes);
	assert_eq!(attention.retry_budget_attempt_count, None);
	assert!(
		attention.summary.contains("Partial worktree changes are retained"),
		"summary should explain retained stalled patch recovery, got {:?}",
		attention.summary
	);
	assert!(
		attention
			.attention_next_action
			.as_deref()
			.is_some_and(|action| action.contains("finish validation and PR handoff"))
	);
}
