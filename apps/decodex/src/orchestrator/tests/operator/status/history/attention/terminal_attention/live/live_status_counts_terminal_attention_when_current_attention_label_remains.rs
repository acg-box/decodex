use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator,
};

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
