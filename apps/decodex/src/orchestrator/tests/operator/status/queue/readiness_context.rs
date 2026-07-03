use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, ProtocolActivityEventSummary, ProtocolActivityMarker,
	ProtocolActivitySummary, RUN_OPERATION_AGENT_RUN, StateStore, fs, orchestrator, state,
};

#[test]
fn live_operator_status_snapshot_reports_ready_when_another_issue_has_active_lease() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let waiting_issue = status::sample_issue_with_sort_fields(
		"issue-waiting",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![waiting_issue]);

	state_store
		.upsert_lease(config.service_id(), "issue-running", "run-active", "In Progress")
		.expect("run lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot.queued_candidates.first().expect("queued issue should exist");

	assert_eq!(candidate.issue_identifier, "PUB-101");
	assert_eq!(candidate.classification, "ready");
	assert_eq!(candidate.reason, "eligible_for_dispatch");
	assert_eq!(candidate.attention, None);
}

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

#[test]
fn live_operator_status_snapshot_surfaces_failed_child_run_after_archive_race() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-109",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("turn_completed")),
		rate_limit_status: None,
		recent_events: vec![ProtocolActivityEventSummary {
			event_type: String::from("thread/archive/discarded"),
			category: String::from("thread"),
			detail: Some(String::from("discarded")),
		}],
	};

	status::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-109", ".worktrees/PUB-109", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained child patch\n")
		.expect("tracked worktree file should change");

	state_store
		.record_run_attempt("run-archive-race", &issue.id, 4, "failed")
		.expect("failed run attempt should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-109",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_event("run-archive-race", 963, "thread/archive/discarded", "{}")
		.expect("archive discard should record");
	state_store
		.append_event(
			"run-archive-race",
			963,
			"item/commandExecution/outputDelta",
			r#"{"delta":"late output"}"#,
		)
		.expect("late output should be discarded without corrupting status");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-archive-race",
			attempt_number: 4,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "thread/archive/discarded",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol marker should write");

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
		.find(|candidate| candidate.issue_identifier == "PUB-109")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.issue_identifier.as_deref() == Some("PUB-109"))
		.expect("retained worktree should remain visible with queue ownership");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(attention.run_id.as_deref(), Some("run-archive-race"));
	assert_eq!(attention.attempt_number, Some(4));
	assert_eq!(attention.attempt_status.as_deref(), Some("failed"));
	assert_eq!(attention.last_event_type.as_deref(), Some("thread/archive/discarded"));
	assert_eq!(attention.event_count, 2);
	assert_eq!(attention.worktree_path.as_deref(), Some(".worktrees/PUB-109"));
	assert!(attention.worktree_has_tracked_changes);
	assert!(attention.summary.contains("Child implementation attempt failed"));
	assert!(attention.summary.contains("parent journal or closeout handling"));
	assert_eq!(worktree.ownership, "orphaned_live_thread");
	assert_eq!(worktree.branch_name, "x/pubfi-pub-109");
	assert_eq!(worktree.worktree_path, ".worktrees/PUB-109");
}

#[test]
fn live_operator_status_snapshot_surfaces_needs_attention_event_cause() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-108",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![status::linear_execution_history_comment(
			&issue,
			"terminal_failure",
			"2026-03-13T09:20:00Z",
			"retained-review-head-mismatch",
			|record| {
				record.error_class = Some(String::from("review_orchestration_head_mismatch"));
				record.next_action = Some(String::from(
					"inspect retained review orchestration reason `review_orchestration_head_mismatch`, resolve the blocker manually",
				));
				record.summary = Some(String::from(
					"Retained review orchestration requires operator attention.",
				));
				record.blockers = Some(vec![String::from(
					"retained review orchestration head mismatch",
				)]);
				record.evidence = Some(vec![String::from(
					"review orchestration marker head differs from local worktree HEAD",
				)]);
				record.terminal_path = Some(String::from("manual_attention"));
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
		.find(|candidate| candidate.issue_identifier == "PUB-108")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(
		attention.attention_error_class.as_deref(),
		Some("review_orchestration_head_mismatch")
	);
	assert_eq!(
		attention.attention_next_action.as_deref(),
		Some(
			"inspect retained review orchestration reason `review_orchestration_head_mismatch`, resolve the blocker manually"
		)
	);
	assert!(rendered.contains("attention_cause: review_orchestration_head_mismatch"));
	assert!(rendered.contains("attention_next_action: inspect retained review orchestration"));
}
