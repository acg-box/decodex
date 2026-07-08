use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn excludes_claimed_candidates_from_waiting_count() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let claimed_issue = status::sample_issue_with_sort_fields(
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
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
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
fn deduplicates_terminal_attention_queue_echo() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-xy-922",
		"XY-922",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-06-11T09:08:00Z",
	);
	let local_comments =
		status::retained_partial_progress_linear_execution_history_comments(&issue);
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

	status::seed_local_linear_execution_events(&state_store, &local_comments);

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
