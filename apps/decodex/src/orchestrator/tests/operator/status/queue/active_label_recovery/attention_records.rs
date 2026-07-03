use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, fs, orchestrator, state, tracker,
};

#[test]
fn live_operator_status_snapshot_preserves_recorded_active_label_attention_next_action() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
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
		vec![status::linear_execution_history_comment(
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
