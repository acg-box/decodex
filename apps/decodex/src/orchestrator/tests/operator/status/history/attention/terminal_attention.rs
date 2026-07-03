use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator, tracker,
};

#[test]
fn local_status_summary_counts_terminal_history_needs_attention_without_queue_candidate() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-xy-922",
		"XY-922",
		"Todo",
		&[],
		Some(3),
		"2026-06-11T09:08:00Z",
	);
	let local_comments =
		status::retained_partial_progress_linear_execution_history_comments(&issue);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"xy/profit-pilot-xy-922",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("retained worktree should be recorded");
	state_store
		.record_run_attempt("xy-922-attempt-1-1781168400", &issue.id, 1, "failed")
		.expect("failed attempt should record");

	status::seed_local_linear_execution_events(&state_store, &local_comments);

	let snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let worktree = snapshot.worktrees.first().expect("retained worktree should render");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(
		snapshot.queued_candidates.is_empty(),
		"terminal ledger attention should not require a queued candidate"
	);
	assert_eq!(snapshot.projects[0].attention_count, 1);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
	assert_eq!(lane.ledger_outcome.ledger_status, "present");
	assert_eq!(lane.ledger_outcome.final_outcome, "needs_attention");
	assert_eq!(
		lane.ledger_outcome.needs_attention_reason.as_deref(),
		Some("Decodex retained validation-ready partial progress for manual review.")
	);
	assert_eq!(lane.latest_run.status, "needs_attention");
	assert_eq!(lane.latest_run.phase, "needs_attention");
	assert_eq!(worktree.ownership, "retained_attention");
	assert_eq!(snapshot_json["projects"][0]["attention_count"], 1);
	assert_eq!(snapshot_json["queued_candidates"].as_array().map(Vec::len), Some(0));
	assert_eq!(snapshot_json["history_lanes"][0]["latest_run"]["status"], "needs_attention");
	assert_eq!(snapshot_json["worktrees"][0]["ownership"], "retained_attention");
	assert!(rendered.contains("outcome: needs_attention"));
	assert!(rendered.contains(
		"needs_attention_reason: Decodex retained validation-ready partial progress for manual review."
	));
	assert!(rendered.contains("role: retained_attention"));
	assert!(rendered.contains("Current attention: 1"));
	assert!(rendered.contains("History-only terminal attention: 0"));
}

#[test]
fn local_status_summary_ignores_history_only_terminal_attention_without_current_owner() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-pub-1549",
		"PUB-1549",
		"Todo",
		&[],
		Some(3),
		"2026-06-12T01:56:00Z",
	);
	let local_comments =
		status::retained_partial_progress_linear_execution_history_comments(&issue);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-mono-pub-1549",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember previous lane ownership");
	state_store
		.record_run_attempt("pub-1549-attempt-1-1781240781", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("historical lane should not have current retained ownership");

	status::seed_local_linear_execution_events(&state_store, &local_comments);

	let snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.current_lanes.len(), 0);
	assert_eq!(snapshot.queued_candidates.len(), 0);
	assert_eq!(snapshot.post_review_lanes.len(), 0);
	assert_eq!(snapshot.worktrees.len(), 0);
	assert_eq!(snapshot.projects[0].current_lane_count, 0);
	assert_eq!(snapshot.projects[0].running_lane_count, 0);
	assert_eq!(snapshot.projects[0].queued_candidate_count, 0);
	assert_eq!(snapshot.projects[0].post_review_lane_count, 0);
	assert_eq!(snapshot.projects[0].attention_count, 0);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 0);
	assert_eq!(snapshot.projects[0].cleanup_blocked_count, 0);
	assert_eq!(snapshot.projects[0].cleanup_pending_count, 0);
	assert_eq!(lane.ledger_outcome.ledger_status, "present");
	assert_eq!(lane.ledger_outcome.final_outcome, "needs_attention");
	assert_eq!(lane.latest_run.status, "needs_attention");
	assert_eq!(lane.latest_run.phase, "needs_attention");
	assert!(!lane.latest_run.run_lease);
	assert_eq!(snapshot_json["projects"][0]["attention_count"], 0);
	assert_eq!(snapshot_json["worktrees"].as_array().map(Vec::len), Some(0));
	assert_eq!(snapshot_json["history_lanes"][0]["latest_run"]["status"], "needs_attention");
	assert!(rendered.contains("Current attention: 0"));
	assert!(rendered.contains("History-only terminal attention: 1"));
	assert!(rendered.contains(
		"Current attention action: none; terminal attention rows below are Run Ledger history only."
	));
	assert!(rendered.contains("outcome: needs_attention"));
}

#[test]
fn live_status_counts_terminal_attention_when_current_attention_label_remains() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-pub-1550",
		"PUB-1550",
		"Todo",
		&["decodex:needs-attention"],
		Some(3),
		"2026-06-12T02:16:00Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![status::linear_execution_history_comment(
			&issue,
			"needs_attention",
			"2026-06-12T02:20:00Z",
			"manual-attention",
			|record| {
				record.summary = Some(String::from("Decodex run requires operator attention."));
				record.error_class = Some(String::from("human_attention_required"));
				record.next_action = Some(String::from(
					"resolve the blocker, clear needs-attention, then requeue if needed",
				));
				record.blockers = Some(vec![String::from("manual blocker remains")]);
				record.evidence = Some(vec![String::from("needs-attention label remains")]);
				record.terminal_path = Some(String::from("manual_attention"));
			},
		)],
	);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-mono-pub-1550",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember previous lane ownership");
	state_store
		.record_run_attempt("pub-1550-attempt-1-1781241600", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	state_store.clear_worktree(&issue.id).expect("current retained worktree should be absent");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(
		snapshot.queued_candidates.iter().all(|candidate| candidate.issue_identifier != "PUB-1550"),
		"terminal attention queue echo should be suppressed"
	);
	assert!(snapshot.worktrees.is_empty());
	assert_eq!(snapshot.projects[0].attention_count, 1);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 0);
	assert_eq!(lane.issue_state.as_deref(), Some("Todo"));
	assert_eq!(lane.needs_attention_label_present, Some(true));
	assert_eq!(lane.ledger_outcome.final_outcome, "needs_attention");
	assert!(rendered.contains("Current attention: 1"));
	assert!(rendered.contains("History-only terminal attention: 0"));
}

#[test]
fn live_status_treats_adopted_ready_to_land_history_attention_as_history_only() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let pr_url = "https://github.com/hack-ink/decodex/pull/360";
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-xy-948",
		"XY-948",
		"In Review",
		&[active_label.as_str()],
		Some(3),
		"2026-06-12T04:20:00Z",
	);

	issue.labels.retain(|label| label.name != queue_label);

	let tracker = FakeTracker::new(vec![issue.clone()]);

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![status::linear_execution_history_comment(
			&issue,
			"needs_attention",
			"2026-06-12T04:30:00Z",
			"manual-attention",
			|record| {
				record.branch = Some(String::from("y/decodex-xy-948"));
				record.worktree_path = Some(String::from(".worktrees/XY-948"));
				record.summary = Some(String::from(
					"Decodex retained validation-ready partial progress for manual review.",
				));
				record.error_class = Some(String::from("partial_progress_retained"));
				record.next_action = Some(String::from(
					"review the retained worktree diff, then commit/push/PR or mark manual disposition",
				));
				record.blockers = Some(vec![String::from(
					"lane stopped before review handoff and terminal finalize",
				)]);
				record.evidence = Some(vec![String::from("cargo make test passed")]);
				record.terminal_path = Some(String::from("retained_partial_progress"));
			},
		)],
	);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/decodex-xy-948",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember previous lane ownership");
	state_store
		.record_run_attempt("xy-948-attempt-1-1781248200", &issue.id, 1, "failed")
		.expect("failed attempt should record");

	let mut snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");

	assert_eq!(
		snapshot.projects[0].attention_count, 1,
		"active label plus retained history should reproduce the pre-adoption current attention signal"
	);
	assert_eq!(snapshot.history_lanes[0].active_label_present, Some(true));
	assert_eq!(snapshot.history_lanes[0].needs_attention_label_present, Some(false));
	assert!(
		snapshot.queued_candidates.iter().all(|candidate| candidate.issue_identifier != "XY-948"),
		"the regression should isolate retained history plus post-review ownership, not queue attention"
	);

	let worktree_path = snapshot.worktrees[0].worktree_path.clone();

	snapshot.post_review_lanes = vec![orchestrator::OperatorPostReviewLaneStatus {
		project_id: TEST_SERVICE_ID.to_owned(),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: issue.state.name.clone(),
		branch_name: String::from("y/decodex-xy-948"),
		worktree_path,
		classification: String::from("ready_to_land"),
		reason: String::from("non_github_review_ready_to_land"),
		pr_url: Some(String::from(pr_url)),
		pr_head_sha: Some(String::from("1111111111111111111111111111111111111111")),
		pr_state: Some(String::from("OPEN")),
		review_decision: Some(String::from("APPROVED")),
		mergeable: Some(String::from("MERGEABLE")),
		check_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: Some(0),
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: None,
		loop_status: None,
	}];

	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();

	orchestrator::refresh_worktree_ownership(&mut snapshot, Some(completed_state));
	orchestrator::refresh_operator_project_summary(&mut snapshot, Some(completed_state));

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.projects[0].attention_count, 0);
	assert_eq!(snapshot.projects[0].post_review_lane_count, 1);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "post_review_lane");
	assert!(rendered.contains("Current attention: 0"));
	assert!(rendered.contains("History-only terminal attention: 1"));
	assert!(rendered.contains("classification: ready_to_land"));
	assert!(rendered.contains("outcome: needs_attention"));
}

#[test]
fn live_status_does_not_count_done_history_attention_without_retained_ownership() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-pub-1549",
		"PUB-1549",
		"Done",
		&[],
		Some(3),
		"2026-06-12T01:56:00Z",
	);

	issue.labels.retain(|label| label.name != queue_label);

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let comments = status::retained_partial_progress_linear_execution_history_comments(&issue);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-mono-pub-1549",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember previous lane ownership");
	state_store
		.record_run_attempt("pub-1549-attempt-1-1781240781", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("completed lane cleanup should clear local worktree");
	tracker.issue_comments.borrow_mut().insert(issue.id.clone(), comments);

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");

	assert_eq!(snapshot.projects[0].attention_count, 0);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 0);
	assert!(snapshot.queued_candidates.is_empty());
	assert_eq!(lane.issue_state.as_deref(), Some("Done"));
	assert_eq!(lane.active_label_present, Some(false));
	assert_eq!(lane.needs_attention_label_present, Some(false));
	assert_eq!(lane.ledger_outcome.final_outcome, "needs_attention");
	assert_eq!(lane.latest_run.status, "needs_attention");
}
