#[test]
fn live_operator_status_snapshot_includes_queued_candidates_with_dispatch_classification() {
	let workflow_markdown =
		sample_workflow_markdown("pubfi", &[], "Follow the repository policy.", 1)
			.replace("max_concurrent_agents = 1", "max_concurrent_agents = 2");
	let (_temp_dir, config, workflow) =
		temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let ready_issue = sample_issue_with_sort_fields(
		"issue-ready",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let mut blocked_issue = sample_issue_with_sort_fields(
		"issue-blocked",
		"PUB-102",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T05:16:17.133Z",
	);

	blocked_issue.description = String::from("```json\n{}\n```");

	let claimed_issue = sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-103",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T06:16:17.133Z",
	);
	let closed_issue = sample_issue_with_sort_fields(
		"issue-closed",
		"PUB-104",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let canceled_issue = sample_issue_with_sort_fields(
		"issue-canceled",
		"PUB-105",
		"Canceled",
		&[],
		Some(5),
		"2026-03-13T08:16:17.133Z",
	);

	state_store
		.upsert_lease(config.service_id(), &claimed_issue.id, "run-claimed", "In Progress")
		.expect("lease should record");

	let tracker = FakeTracker::new(vec![
		claimed_issue.clone(),
		blocked_issue.clone(),
		closed_issue.clone(),
		canceled_issue.clone(),
		ready_issue.clone(),
	]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");

	assert_eq!(snapshot.queued_candidates.len(), 3);

	let queued_by_issue = snapshot
		.queued_candidates
		.iter()
		.map(|candidate| (candidate.issue_identifier.as_str(), candidate))
		.collect::<HashMap<_, _>>();

	assert_eq!(
		queued_by_issue.get("PUB-101").expect("ready queued issue should exist").classification,
		"ready"
	);
	assert_eq!(
		queued_by_issue.get("PUB-101").expect("ready queued issue should exist").reason,
		"eligible_for_dispatch"
	);
	assert_eq!(
		queued_by_issue.get("PUB-102").expect("blocked queued issue should exist").classification,
		"blocked"
	);
	assert_eq!(
		queued_by_issue.get("PUB-102").expect("blocked queued issue should exist").reason,
		"missing_dispatch_briefing"
	);
	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").classification,
		"claimed"
	);
	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").reason,
		"shared_claim_present"
	);
	assert!(
		!queued_by_issue.contains_key("PUB-104"),
		"terminal queued echoes should not appear in operator intake candidates"
	);
	assert!(
		!queued_by_issue.contains_key("PUB-105"),
		"canceled queued echoes should not appear in operator intake candidates"
	);
}

#[test]
fn live_operator_status_snapshot_routes_retained_success_state_lane_out_of_queue() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = sample_issue_with_sort_fields(
		"issue-review",
		"PUB-106",
		"In Review",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	issue.blockers = vec![sample_blocker("issue-done", "PUB-105", "Done")];

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-106",
			&worktree_path.display().to_string(),
		)
		.expect("retained review worktree should record");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let lane = snapshot
		.post_review_lanes
		.iter()
		.find(|lane| lane.issue_identifier == "PUB-106")
		.expect("retained success-state worktree should be owned by post-review status");

	assert!(
		snapshot
			.queued_candidates
			.iter()
			.all(|candidate| candidate.issue_identifier != "PUB-106"),
		"post-review retained lanes must not also appear as queue intake blockers"
	);
	assert_eq!(lane.reason, "missing_review_handoff_record");
	assert_eq!(
		project.queued_candidate_count, 0,
		"post-review retained lanes must not inflate intake backlog"
	);
}

#[test]
fn live_operator_status_snapshot_reports_only_open_tracker_blockers() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = sample_issue_with_sort_fields(
		"issue-blocked",
		"PUB-107",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);

	issue.blockers = vec![
		sample_blocker("issue-done", "PUB-106", "Done"),
		sample_blocker("issue-open", "PUB-105", "Todo"),
	];

	let tracker = FakeTracker::new(vec![issue]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate =
		snapshot.queued_candidates.first().expect("blocked queued issue should exist");

	assert_eq!(candidate.reason, "open_tracker_blockers");
	assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
}

#[test]
fn live_operator_status_snapshot_marks_repeated_open_blockers_as_stale_program() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = sample_issue_with_sort_fields(
		"issue-blocked",
		"PUB-108",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);

	issue.blockers = vec![sample_blocker("issue-open", "PUB-105", "Todo")];

	for expected_reason in [
		"open_tracker_blockers",
		"open_tracker_blockers",
		"dependency_program_stale",
	] {
		let tracker = FakeTracker::new(vec![issue.clone()]);
		let snapshot = orchestrator::build_live_operator_status_snapshot(
			&tracker,
			&config,
			&workflow,
			&state_store,
			10,
		)
		.expect("snapshot should build");
		let candidate =
			snapshot.queued_candidates.first().expect("blocked queued issue should exist");

		assert_eq!(candidate.reason, expected_reason);
		assert_eq!(candidate.classification, "blocked");
		assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
	}

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "dependency_program_stale")
		.expect("dependency guardrail checkpoint should read")
		.expect("dependency guardrail checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);

	issue.blockers = vec![sample_blocker("issue-open", "PUB-105", "Done")];

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("resolved blocker snapshot should build");
	let candidate =
		snapshot.queued_candidates.first().expect("ready queued issue should exist");

	assert_eq!(candidate.reason, "eligible_for_dispatch");
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "dependency_program_stale")
			.expect("cleared dependency checkpoint should read")
			.is_none()
	);

	issue.blockers = vec![sample_blocker("issue-open", "PUB-105", "Todo")];

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("recurring blocker snapshot should build");
	let candidate =
		snapshot.queued_candidates.first().expect("blocked queued issue should exist");

	assert_eq!(candidate.reason, "open_tracker_blockers");
	assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
}

#[test]
fn live_operator_status_snapshot_excludes_claimed_candidates_from_waiting_intake_count() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let claimed_issue = sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-103",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T06:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![claimed_issue.clone()]);

	state_store
		.record_run_attempt("run-claimed", &claimed_issue.id, 1, "running")
		.expect("active run should record");
	state_store
		.upsert_lease(config.service_id(), &claimed_issue.id, "run-claimed", "In Progress")
		.expect("active lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let candidate =
		snapshot.queued_candidates.first().expect("claimed queue echo should remain raw-visible");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.active_runs.len(), 1);
	assert_eq!(snapshot.active_runs[0].run_id, "run-claimed");
	assert_eq!(project.active_run_count, 1);
	assert_eq!(candidate.issue_identifier, "PUB-103");
	assert_eq!(candidate.classification, "claimed");
	assert_eq!(candidate.reason, "shared_claim_present");
	assert_eq!(
		project.queued_candidate_count, 0,
		"claimed queue echoes are raw state, not waiting intake"
	);
	assert_eq!(
		project.waiting_lane_count, 0,
		"claimed queue echoes must not inflate project waiting counts"
	);
	assert!(rendered.contains("Backlog: 0"));
	assert!(rendered.contains("Active queue echoes: 1"));
}

#[test]
fn live_operator_status_snapshot_prioritizes_needs_attention_over_shared_claim() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-attention-claimed",
		"PUB-113",
		"Todo",
		&["decodex:needs-attention"],
		Some(3),
		"2026-03-13T06:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.record_run_attempt("run-attention-claimed", &issue.id, 1, "running")
		.expect("active run should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "run-attention-claimed", "In Progress")
		.expect("active lease should record");

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
		.find(|candidate| candidate.issue_identifier == "PUB-113")
		.expect("needs-attention claimed issue should remain visible");
	let attention = candidate.attention.as_ref().expect("attention details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(
		attention.auto_retry_blocked_reason.as_deref(),
		Some("needs_attention_label")
	);
	assert_eq!(project.attention_count, 1);
	assert_eq!(
		project.queued_candidate_count, 1,
		"needs-attention queue echoes remain in blocked intake while also counting as attention"
	);
}

#[test]
fn live_operator_status_snapshot_blocks_active_plus_queued_label_without_local_claim() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
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
		vec![linear_execution_history_comment(
			&issue,
			"needs_attention",
			"2026-03-13T04:20:00Z",
			"older-attention",
			|record| {
				record.error_class = Some(String::from("older_attention_record"));
				record.summary = Some(String::from("Older attention record should not mask liveness."));
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
	assert_eq!(
		attention.auto_retry_blocked_reason.as_deref(),
		Some("linear_active_label_present")
	);
	assert_eq!(attention.attention_error_class.as_deref(), Some("evidence_missing"));
	assert_eq!(attention.process_alive, Some(false));
	assert_eq!(attention.process_liveness_reason.as_deref(), Some("process_stopped"));
	assert_eq!(project.attention_count, 1);
	assert!(rendered.contains("reason: linear_active_label_present"));
	assert!(rendered.contains("attention_cause: evidence_missing"));
}

#[test]
fn live_operator_status_snapshot_surfaces_dirty_active_label_recovery_worktree() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-dirty-active",
		"PUB-112",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	git_status_success(
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
fn live_operator_status_snapshot_reports_capacity_waiting_separately_from_blocked() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let waiting_issue = sample_issue_with_sort_fields(
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
		.expect("active lease should consume the single global slot");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot.queued_candidates.first().expect("waiting queued issue should exist");

	assert_eq!(candidate.issue_identifier, "PUB-101");
	assert_eq!(candidate.classification, "waiting");
	assert_eq!(candidate.reason, "global_concurrency_exhausted");
	assert_eq!(candidate.attention, None);
}

#[test]
fn live_operator_status_snapshot_includes_needs_attention_run_context() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
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
fn live_operator_status_snapshot_surfaces_needs_attention_event_cause() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
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
		vec![linear_execution_history_comment(
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

#[test]
fn live_operator_status_snapshot_surfaces_plugin_list_preflight_timeout() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-109",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![linear_execution_history_comment(
			&issue,
			"terminal_failure",
			"2026-03-13T09:20:00Z",
			"app-server-plugin-list-timeout",
			|record| {
				record.error_class = Some(String::from("app_server_plugin_list_timeout"));
				record.next_action = Some(String::from(
					"inspect local app_server_preflight_failed evidence for the `plugin/list` timeout, restart `decodex serve`, run `decodex probe`, clear label `decodex:needs-attention`",
				));
				record.summary = Some(String::from("Decodex run failed and needs attention."));
				record.blockers = Some(vec![String::from(
					"plugin/list timed out during app-server preflight",
				)]);
				record.evidence = Some(vec![String::from(
					"app_server_preflight_failed happened before thread/start",
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
		.find(|candidate| candidate.issue_identifier == "PUB-109")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(attention.attention_error_class.as_deref(), Some("app_server_plugin_list_timeout"));
	assert!(attention.summary.contains("app_server_preflight_failed: plugin/list timed out"));
	assert!(rendered.contains("attention_cause: app_server_plugin_list_timeout"));
	assert!(rendered.contains("attention_next_action: inspect local app_server_preflight_failed"));
	assert!(rendered.contains("plugin/list"));
}

#[test]
fn live_operator_status_snapshot_surfaces_retained_partial_progress() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-106",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	git_status_success(
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-110",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-110", ".worktrees/PUB-110", "main"],
	);

	fs::write(worktree_path.join("README.md"), "retained stalled patch\n")
		.expect("tracked worktree file should change");

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
			vec![linear_execution_history_comment(
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

#[test]
fn live_operator_status_snapshot_surfaces_git_credential_failures() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-105",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state::write_run_operation_marker(
		&worktree_path,
		"run-missing-credentials",
		1,
		RUN_OPERATION_GIT_CREDENTIALS,
	)
	.expect("credential preflight marker should write");

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

	assert!(snapshot.active_runs.is_empty());
	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(attention.current_operation.as_deref(), Some(state::RUN_OPERATION_GIT_CREDENTIALS));
	assert_eq!(attention.summary, "Git credential preflight failed; operator recovery required.");
}

#[test]
fn live_operator_status_snapshot_recovers_shared_claims_for_fresh_status_store_instances() {
	let workflow_markdown =
		sample_workflow_markdown("pubfi", &[], "Follow the repository policy.", 1)
			.replace("max_concurrent_agents = 1", "max_concurrent_agents = 2");
	let (_temp_dir, config, workflow) =
		temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let remote_store = StateStore::open_in_memory().expect("remote state store should open");
	let observer_store = StateStore::open_in_memory().expect("observer state store should open");
	let claimed_issue = sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-103",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T06:16:17.133Z",
	);
	let ready_issue = sample_issue_with_sort_fields(
		"issue-ready",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![claimed_issue.clone(), ready_issue]);

	remote_store
		.configure_dispatch_slot_root(
			config.service_id(),
			config.worktree_root(),
			workflow.frontmatter().execution().max_concurrent_agents(),
		)
		.expect("remote store should configure dispatch-slot root");

	assert!(
		remote_store
			.try_acquire_lease(
				config.service_id(),
				&claimed_issue.id,
				"run-claimed",
				workflow.frontmatter().tracker().in_progress_state(),
			)
			.expect("remote store should acquire the shared issue claim")
	);

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&observer_store,
		10,
	)
	.expect("snapshot should build");
	let queued_by_issue = snapshot
		.queued_candidates
		.iter()
		.map(|candidate| (candidate.issue_identifier.as_str(), candidate))
		.collect::<HashMap<_, _>>();

	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").classification,
		"claimed"
	);
	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").reason,
		"shared_claim_present"
	);
	assert!(
		snapshot.active_runs.is_empty(),
		"fresh observer stores should not invent local running lanes while reconstructing the shared claim view"
	);
}

#[test]
fn live_operator_status_snapshot_reconstructs_same_shared_view_for_fresh_state_stores() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_issue = sample_active_issue("In Progress");
	let closed_issue = sample_issue_with_sort_fields(
		"issue-closed",
		"PUB-104",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![active_issue.clone(), closed_issue]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&active_issue.identifier, false)
		.expect("retained worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-1", 1)
		.expect("activity marker should write");

	let build_view = |state_store: &StateStore| -> Value {
		let recovered = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
			&tracker,
			&config,
			&workflow,
			state_store,
		)
		.expect("runtime recovery should succeed");

		orchestrator::hydrate_status_snapshot_state(&config, state_store, recovered)
			.expect("status hydration should succeed");

		let snapshot = orchestrator::build_live_operator_status_snapshot(
			&tracker,
			&config,
			&workflow,
			state_store,
			10,
		)
		.expect("snapshot should build");

		serde_json::json!({
			"active_runs": snapshot.active_runs.iter().map(|run| {
				serde_json::json!({
					"run_id": run.run_id,
					"issue_id": run.issue_id,
					"phase": run.phase,
					"current_operation": run.current_operation,
					"active_lease": run.active_lease,
					"branch_name": run.branch_name,
					"worktree_path": run.worktree_path,
				})
			}).collect::<Vec<_>>(),
			"queued_candidates": snapshot.queued_candidates,
			"worktrees": snapshot.worktrees,
			"post_review_lanes": snapshot.post_review_lanes,
		})
	};
	let first_store = StateStore::open_in_memory().expect("first state store should open");
	let second_store = StateStore::open_in_memory().expect("second state store should open");

	assert_eq!(build_view(&first_store), build_view(&second_store));
}
