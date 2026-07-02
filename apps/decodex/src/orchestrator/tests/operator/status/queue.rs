use super::*;
use crate::orchestrator::tests::recovery_terminal_support;

#[test]
fn live_operator_status_snapshot_includes_queued_candidates_with_dispatch_classification() {
	let workflow_markdown =
		sample_workflow_markdown("pubfi", &[], "Follow the repository policy.", 1);
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
		snapshot.queued_candidates.iter().all(|candidate| candidate.issue_identifier != "PUB-106"),
		"post-review retained lanes must not also appear as queue intake blockers"
	);
	assert_eq!(lane.reason, "missing_review_handoff_record");
	assert_eq!(
		project.queued_candidate_count, 0,
		"post-review retained lanes must not inflate intake backlog"
	);
}

#[test]
fn live_operator_status_snapshot_blocks_ordinary_queue_for_retained_handoff_marker() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = sample_issue_with_sort_fields(
		"issue-review",
		"PUB-106",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker = FakeTracker::new(vec![issue]);
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
		.expect("retained handoff queue candidate should stay visible");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "review_handoff_state_transition_pending");
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
	let candidate = snapshot.queued_candidates.first().expect("blocked queued issue should exist");

	assert_eq!(candidate.reason, "open_tracker_blockers");
	assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
}

#[test]
#[allow(clippy::too_many_lines)]
fn queued_status_guardrail_requires_explicit_command_application() {
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

	for _ in 0..3 {
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

		assert_eq!(candidate.reason, "open_tracker_blockers");
		assert_eq!(candidate.classification, "blocked");
		assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
	}

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "dependency_program_stale")
			.expect("dependency guardrail checkpoint should read")
			.is_none(),
		"operator status reads must not mutate queue guardrail checkpoints"
	);

	for _ in 0..3 {
		let tracker = FakeTracker::new(vec![issue.clone()]);
		let plan = orchestrator::build_queued_candidate_status_plan(
			&tracker,
			&config,
			&workflow,
			&state_store,
		)
		.expect("queued status plan should build");
		let command = plan
			.guardrail_commands
			.first()
			.expect("open blockers should request a guardrail observation");

		assert_eq!(command.intent.kind.as_str(), "observe_loop_guardrail_checkpoint");
		assert_eq!(
			command.intent.idempotency_key,
			"issue-blocked:dependency_program_stale:observe"
		);
		assert_eq!(
			command.intent.preconditions.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
			vec!["open_tracker_blockers_present"]
		);
		assert_eq!(
			command
				.intent
				.expected_postconditions
				.iter()
				.map(|fact| fact.as_str())
				.collect::<Vec<_>>(),
			vec!["loop_guardrail_checkpoint_observed"]
		);

		orchestrator::apply_queued_candidate_guardrail_commands(
			&config,
			&workflow,
			&state_store,
			&plan.guardrail_commands,
		)
		.expect("guardrail command application should succeed");
	}

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "dependency_program_stale")
		.expect("dependency guardrail checkpoint should read")
		.expect("dependency guardrail checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("stale blocker snapshot should build");
	let candidate = snapshot.queued_candidates.first().expect("blocked queued issue should exist");

	assert_eq!(candidate.reason, "dependency_program_stale");

	issue.blockers = vec![sample_blocker("issue-open", "PUB-105", "Done")];

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let plan = orchestrator::build_queued_candidate_status_plan(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("resolved blocker plan should build");
	let candidate = plan.statuses.first().expect("ready queued issue should exist");
	let command = plan
		.guardrail_commands
		.first()
		.expect("resolved blockers should request stale guardrail cleanup");

	assert_eq!(candidate.reason, "eligible_for_dispatch");
	assert_eq!(command.intent.kind.as_str(), "clear_loop_guardrail_checkpoint");
	assert_eq!(command.intent.idempotency_key, "issue-blocked:dependency_program_stale:clear");
	assert_eq!(
		command.intent.preconditions.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
		vec!["open_tracker_blockers_resolved"]
	);
	assert_eq!(
		command.intent.expected_postconditions.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
		vec!["loop_guardrail_checkpoint_cleared"]
	);

	orchestrator::apply_queued_candidate_guardrail_commands(
		&config,
		&workflow,
		&state_store,
		&plan.guardrail_commands,
	)
	.expect("clear guardrail command should apply");

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
	let candidate = snapshot.queued_candidates.first().expect("blocked queued issue should exist");

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
		.expect("current lane should record");
	state_store
		.upsert_lease(config.service_id(), &claimed_issue.id, "run-claimed", "In Progress")
		.expect("run lease should record");

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

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, "run-claimed");
	assert_eq!(project.current_lane_count, 1);
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
	assert!(rendered.contains("Claimed queue echoes: 1"));
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
		.expect("current lane should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "run-attention-claimed", "In Progress")
		.expect("run lease should record");

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
	assert_eq!(attention.auto_retry_blocked_reason.as_deref(), Some("needs_attention_label"));
	assert_eq!(project.attention_count, 1);
	assert_eq!(
		project.queued_candidate_count, 1,
		"needs-attention queue echoes remain in blocked intake while also counting as attention"
	);
}

#[test]
fn live_operator_status_snapshot_deduplicates_terminal_retained_attention_queue_echo() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-xy-922",
		"XY-922",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-06-11T09:08:00Z",
	);
	let local_comments = retained_partial_progress_linear_execution_history_comments(&issue);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	tracker.issue_comments.borrow_mut().insert(issue.id.clone(), local_comments.clone());
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"xy/profit-pilot-xy-922",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("retained worktree should record");
	state_store
		.record_run_attempt("xy-355-attempt-1-1777527013", &issue.id, 1, "failed")
		.expect("failed run attempt should record");

	seed_local_linear_execution_events(&state_store, &local_comments);

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
		.history_lanes
		.iter()
		.find(|lane| lane.issue_key == "XY-922")
		.expect("terminal retained lane should render from run ledger");
	let worktree = snapshot.worktrees.first().expect("retained worktree should render");

	assert!(
		snapshot.queued_candidates.iter().all(|candidate| candidate.issue_identifier != "XY-922"),
		"terminal retained attention should not remain as an intake queue candidate"
	);
	assert_eq!(project.queued_candidate_count, 0);
	assert_eq!(project.attention_count, 1);
	assert_eq!(project.retained_worktree_count, 1);
	assert_eq!(
		lane.ledger_outcome.needs_attention_reason.as_deref(),
		Some("Decodex retained validation-ready partial progress for manual review.")
	);
	assert_eq!(worktree.ownership, "retained_attention");
	assert!(
		worktree
			.recovery_next_action
			.as_deref()
			.is_some_and(|next_action| next_action.contains("validation-ready partial progress")),
		"retained worktree next action should come from the terminal run ledger"
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
fn live_operator_status_snapshot_preserves_recorded_active_label_attention_next_action() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-active-recorded-attention",
		"PUB-113",
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
			"recorded-attention",
			|record| {
				record.error_class = Some(String::from("review_policy_checkpoint_present"));
				record.summary = Some(String::from("Retained review evidence is present."));
				record.next_action = Some(String::from("resume_review_handoff_recovery"));
				record.blockers = Some(vec![String::from("review_policy_checkpoint_present")]);
				record.evidence = Some(vec![String::from("review checkpoint")]);
				record.terminal_path = Some(String::from("manual_attention"));
			},
		)],
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-113",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.record_run_attempt("pub-113-attempt-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "pub-113-attempt-1", 1, u32::MAX)
		.expect("stopped process marker should write");
	state_store
		.append_private_execution_event(
			config.service_id(),
			&issue.id,
			"pub-113-attempt-1",
			1,
			"review_policy_checkpoint",
			serde_json::json!({"phase": "review"}),
		)
		.expect("private evidence should record");

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
		.expect("active issue should remain visible");
	let attention = candidate.attention.as_ref().expect("recovery details should render");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "linear_active_label_present");
	assert_eq!(
		attention.attention_error_class.as_deref(),
		Some("review_policy_checkpoint_present")
	);
	assert_eq!(attention.attention_next_action.as_deref(), Some("resume_review_handoff_recovery"));
}

#[test]
fn live_operator_status_snapshot_distinguishes_clean_failed_start_active_cleanup_debt() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-untracked-active",
		"PUB-114",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	git_status_success(
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-unreadable-marker-active",
		"PUB-116",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	fs::create_dir_all(worktree_path.join(state::RUN_ACTIVITY_MARKER_FILE))
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
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

#[test]
fn live_operator_status_snapshot_reports_ready_when_another_issue_has_active_lease() {
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
fn live_operator_status_snapshot_surfaces_failed_child_run_after_archive_race() {
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

	git_status_success(
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
fn live_operator_status_snapshot_surfaces_authority_decision_request() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-decision-request",
		"PUB-118",
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
			"needs_attention",
			"2026-03-13T09:20:00Z",
			"contract-boundary-required",
			|record| {
				record.error_class = Some(String::from("contract_boundary_required"));
				record.next_action = Some(String::from(
					"accept, reject, or revise decision request `dr-pub-118-1`, then clear needs-attention and requeue through Decodex",
				));
				record.summary =
					Some(String::from("Authority boundary requires a human decision."));
				record.blockers = Some(vec![String::from(
					"accepted behavior change exceeds current authority",
				)]);
				record.evidence = Some(vec![String::from(
					"authority boundary check requires human direction",
				)]);
				record.terminal_path = Some(String::from("manual_attention"));
			},
		)],
	);

	let boundary_event = orchestrator::record_authority_boundary_check_private_event(
		&state_store,
		AuthorityBoundaryCheckInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "xy-355-attempt-1-1777527013",
			attempt_number: 1,
			decision_contract_ids: vec!["contract-pub-118"],
			attempted_recovery_reason: "uncovered_direction",
			changed_surfaces: vec![AuthorityBoundaryChangedSurface {
				surface: AuthorityBoundarySurface::Objective,
				change_summary: "Public behavior would change.",
				policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
				legacy_disposition: AuthorityBoundaryDisposition::RequiresHuman,
			}],
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_disposition_reason: "Accepted behavior needs explicit authority.",
			improvement_signals: Vec::new(),
		},
	)
	.expect("boundary event should persist");

	orchestrator::record_authority_decision_request_private_event(
		&state_store,
		AuthorityDecisionRequestInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "xy-355-attempt-1-1777527013",
			attempt_number: 1,
			boundary_check_record_id: boundary_event.record_id(),
			decision_request_id: "dr-pub-118-1",
			reason_code: "contract_boundary_required",
			boundary_type: "accepted_behavior",
			proposed_change: "Change accepted operator behavior.",
			why_exceeds_authority: "The current issue did not authorize the behavior change.",
			options: vec![orchestrator::AuthorityDecisionOption {
				label: "revise",
				description: "Update the Decision Contract before resuming.",
			}],
			recommendation: "Revise the Decision Contract before resuming automation.",
			resume_condition: "Clear needs-attention and requeue only after authority is updated.",
			retained_worktree_evidence: vec!["retained worktree has tracked changes"],
			retained_diff_evidence: vec!["private diff summary retained locally"],
			recovery_attempt_context: vec!["recovery stopped at the authority boundary"],
		},
	)
	.expect("decision request should persist");

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
		.find(|candidate| candidate.issue_identifier == "PUB-118")
		.expect("needs-attention queued issue should exist");
	let decision_request = candidate
		.attention
		.as_ref()
		.and_then(|attention| attention.decision_request.as_ref())
		.expect("decision request should render");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(decision_request.phase, "human_required");
	assert_eq!(decision_request.reason, "contract_boundary_required");
	assert_eq!(decision_request.boundary, "accepted_behavior");
	assert_eq!(decision_request.decision_request_id, "dr-pub-118-1");
	assert!(rendered.contains("decision_request_phase: human_required"));
	assert!(rendered.contains("decision_request_id: dr-pub-118-1"));
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

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(attention.current_operation.as_deref(), Some(state::RUN_OPERATION_GIT_CREDENTIALS));
	assert_eq!(attention.summary, "Git credential preflight failed; operator recovery required.");
}

#[test]
fn live_operator_status_snapshot_recovers_shared_claims_for_fresh_status_store_instances() {
	let workflow_markdown =
		sample_workflow_markdown("pubfi", &[], "Follow the repository policy.", 1);
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
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
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
		snapshot.current_lanes.is_empty(),
		"fresh observer stores should not invent local running lanes while reconstructing the shared claim view"
	);
}

#[test]
fn live_operator_status_snapshot_reconstructs_same_shared_view_for_fresh_state_stores() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_issue = recovery_terminal_support::sample_active_issue("In Progress");
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
			"current_lanes": snapshot.current_lanes.iter().map(|run| {
				serde_json::json!({
					"run_id": run.run_id,
					"issue_id": run.issue_id,
					"phase": run.phase,
					"current_operation": run.current_operation,
					"run_lease": run.run_lease,
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
