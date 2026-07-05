use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, RUN_OPERATION_AGENT_RUN, StateStore, orchestrator, state,
};

#[test]
fn live_operator_status_snapshot_includes_needs_attention_run_context() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-105",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-needs-attention",
		3,
		Some("thread-1"),
		Some("turn-1"),
		"systemError",
		&[],
	)
	.expect("thread status marker should write");
	state::write_run_retry_budget_attempt_count(&worktree_path, "run-needs-attention", 3, 3)
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
		.find(|candidate| candidate.issue_identifier == "PUB-105")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(attention.run_id.as_deref(), Some("run-needs-attention"));
	assert_eq!(attention.attempt_number, Some(3));
	assert_eq!(attention.current_operation.as_deref(), Some(state::RUN_OPERATION_AGENT_RUN));
	assert_eq!(attention.thread_status.as_deref(), Some("systemError"));
	assert_eq!(attention.attempt_status, None);
	assert_eq!(attention.retry_budget_attempt_count, Some(3));
	assert_eq!(attention.retry_budget_max_attempts, 3);
	assert_eq!(attention.worktree_path.as_deref(), Some(".worktrees/PUB-105"));
	assert!(attention.summary.contains("systemError"));
	assert!(
		snapshot.worktrees.iter().any(|worktree| worktree.worktree_path == ".worktrees/PUB-105"),
		"needs-attention worktree should still be reported in raw snapshot state"
	);
	assert_eq!(
		snapshot.projects[0].retained_worktree_count, 0,
		"needs-attention queue ownership should keep the worktree out of recovery cleanup counts"
	);

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("attention_worktree: .worktrees/PUB-105"));
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("- none (owned worktrees are shown in their lane sections above)"));
	assert!(!rendered.contains("role: cleanup_only"));
}

#[test]
fn live_operator_status_snapshot_explains_needs_attention_before_retry_budget() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-107",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state::write_run_operation_marker(
		&worktree_path,
		"run-needs-attention",
		1,
		RUN_OPERATION_AGENT_RUN,
	)
	.expect("operation marker should write");
	state::write_run_retry_budget_attempt_count(&worktree_path, "run-needs-attention", 1, 1)
		.expect("retry budget marker should write");

	state_store
		.record_run_attempt("run-needs-attention", &issue.id, 1, "interrupted")
		.expect("interrupted attempt should record");

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
		.find(|candidate| candidate.issue_identifier == "PUB-107")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(attention.attempt_status.as_deref(), Some("interrupted"));
	assert_eq!(attention.auto_retry_blocked_reason.as_deref(), Some("needs_attention_label"));
	assert_eq!(attention.retry_budget_attempt_count, Some(1));
	assert_eq!(attention.retry_budget_max_attempts, 3);
	assert_eq!(
		attention.summary,
		"Previous attempt was interrupted during agent execution; operator recovery required."
	);
}
