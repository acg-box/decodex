use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator, tracker,
};

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
