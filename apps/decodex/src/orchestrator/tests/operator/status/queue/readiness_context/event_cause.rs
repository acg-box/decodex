use crate::orchestrator::tests::operator::status::{self, FakeTracker, StateStore, orchestrator};

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
				record.error_class = Some(String::from("review_lifecycle_authority_head_mismatch"));
				record.next_action = Some(String::from(
					"inspect retained review lifecycle reason `review_lifecycle_authority_head_mismatch`, resolve the blocker manually",
				));
				record.summary = Some(String::from(
					"Retained review orchestration requires operator attention.",
				));
				record.blockers = Some(vec![String::from(
					"retained review orchestration head mismatch",
				)]);
				record.evidence = Some(vec![String::from(
					"review lifecycle transition fixture head differs from local worktree HEAD",
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
		Some("review_lifecycle_authority_head_mismatch")
	);
	assert_eq!(
		attention.attention_next_action.as_deref(),
		Some(
			"inspect retained review lifecycle reason `review_lifecycle_authority_head_mismatch`, resolve the blocker manually"
		)
	);
	assert!(rendered.contains("attention_cause: review_lifecycle_authority_head_mismatch"));
	assert!(
		rendered.contains(
			"attention_next_action: inspect retained review lifecycle reason `review_lifecycle_authority_head_mismatch`, resolve the blocker manually"
		)
	);
}
