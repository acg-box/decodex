use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, fs, orchestrator, state, tracker,
};

#[test]
fn blocks_active_plus_queued_label_without_claim() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-active-queued",
		"PUB-111",
		"Todo",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![status::linear_execution_history_comment(
			&issue,
			"needs_attention",
			"2026-03-13T04:20:00Z",
			"older-attention",
			|record| {
				record.error_class = Some(String::from("older_attention_record"));
				record.summary =
					Some(String::from("Older attention record should not mask liveness."));
				record.next_action = Some(String::from("Reconcile the retained lane."));
				record.blockers = Some(Vec::new());
				record.evidence = Some(vec![String::from("older attention event")]);
				record.terminal_path = Some(String::from("manual_attention"));
			},
		)],
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-111",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.record_run_attempt("pub-111-attempt-1", &issue.id, 1, "running")
		.expect("run attempt should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "pub-111-attempt-1", 1, u32::MAX)
		.expect("stopped process marker should write");

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
		.find(|candidate| candidate.issue_identifier == "PUB-111")
		.expect("active-plus-queued issue should remain visible");
	let attention = candidate.attention.as_ref().expect("recovery details should render");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "linear_active_label_present");
	assert_eq!(attention.auto_retry_blocked_reason.as_deref(), Some("linear_active_label_present"));
	assert_eq!(attention.attention_error_class.as_deref(), Some("evidence_missing"));
	assert!(
		attention
			.attention_next_action
			.as_deref()
			.is_some_and(|action| action.contains("run_stale_active_recovery")
				&& action.contains("recover stale-active release PUB-111 --dry-run")),
		"stale active blocker should point to supported recovery, got {:?}",
		attention.attention_next_action
	);
	assert_eq!(attention.process_alive, Some(false));
	assert_eq!(attention.process_liveness_reason.as_deref(), Some("process_stopped"));
	assert_eq!(project.attention_count, 1);
	assert!(rendered.contains("reason: linear_active_label_present"));
	assert!(rendered.contains("attention_cause: evidence_missing"));
	assert!(rendered.contains("attention_next_action: run_stale_active_recovery"));
}

#[test]
fn distinguishes_clean_failed_start_cleanup_debt() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-failed-start-active",
		"PUB-112",
		"Todo",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-112",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.record_run_attempt("pub-112-attempt-1", &issue.id, 1, "failed")
		.expect("failed attempt should record");

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
		.find(|candidate| candidate.issue_identifier == "PUB-112")
		.expect("active cleanup debt should remain visible");
	let attention = candidate.attention.as_ref().expect("attention details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "linear_active_label_present");
	assert!(!attention.worktree_has_tracked_changes);
	assert_eq!(attention.retry_budget_attempt_count, Some(1));
	assert!(
		attention.summary.contains("Retryable failed-start cleanup is still pending"),
		"summary should distinguish clean failed-start cleanup debt, got {:?}",
		attention.summary
	);
	assert!(
		!attention.summary.contains("Partial worktree changes are retained"),
		"clean failed-start cleanup debt must not look like retained partial progress"
	);
}
