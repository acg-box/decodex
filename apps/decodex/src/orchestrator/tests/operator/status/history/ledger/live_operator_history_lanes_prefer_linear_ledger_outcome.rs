use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn live_operator_history_lanes_prefer_linear_ledger_outcome() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-355",
		"Done",
		&[],
		Some(3),
		"2026-04-29T10:11:00Z",
	);

	issue.title = String::from("Keep completed run rows self describing");

	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/decodex-xy-355",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember project ownership");
	state_store
		.record_run_attempt("xy-355-attempt-1-1777527013", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	state_store
		.record_run_attempt("xy-355-attempt-2-1777527613", &issue.id, 2, "failed")
		.expect("stale failed attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("completed lane cleanup should clear local worktree");
	tracker
		.issue_comments
		.borrow_mut()
		.insert(issue.id.clone(), status::successful_linear_execution_history_comments(&issue));

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let outcome_index = rendered.find("outcome: closeout").expect("ledger outcome should render");
	let local_index = rendered.find("latest_run_id:").expect("local attempt debug should render");

	assert!(snapshot.recent_runs.is_empty());
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert!(lane.attempts.iter().all(|run| run.project_id == TEST_SERVICE_ID));
	assert!(lane.attempts.iter().all(|run| {
		run.issue_identifier.as_deref() == Some("XY-355")
			&& run.title.as_deref() == Some("Keep completed run rows self describing")
	}));
	assert_eq!(lane.project_id, TEST_SERVICE_ID);
	assert_eq!(lane.issue_identifier.as_deref(), Some("XY-355"));
	assert_eq!(lane.title.as_deref(), Some("Keep completed run rows self describing"));
	assert_eq!(lane.latest_run.issue_identifier.as_deref(), Some("XY-355"));
	assert_eq!(lane.latest_run.title.as_deref(), Some("Keep completed run rows self describing"));
	assert_eq!(lane.latest_run.status, "closeout");
	assert_eq!(lane.latest_run.attempt_status, "closeout");
	assert_eq!(lane.latest_run.phase, "completed");
	assert_eq!(lane.latest_run.current_operation, "ledger_outcome");
	assert!(lane.attempts.iter().any(|attempt| attempt.status == "failed"));
	assert_eq!(lane.ledger_outcome.ledger_status, "present");
	assert_eq!(lane.ledger_outcome.final_outcome, "closeout");
	assert_eq!(
		lane.ledger_outcome.pr_url.as_deref(),
		Some("https://github.com/hack-ink/decodex/pull/355")
	);
	assert_eq!(
		lane.ledger_outcome.commit_sha.as_deref(),
		Some("2222222222222222222222222222222222222222")
	);
	assert_eq!(lane.ledger_outcome.closeout_status.as_deref(), Some("Done"));
	assert_eq!(lane.ledger_outcome.needs_attention_reason, None);
	assert_eq!(lane.ledger_outcome.lifecycle_elapsed_seconds, Some(600));
	assert!(
		outcome_index < local_index,
		"durable ledger outcome should be primary before local attempt details"
	);
	assert!(rendered.contains("ledger_status: present"));
	assert!(rendered.contains("pr_url: https://github.com/hack-ink/decodex/pull/355"));
	assert!(rendered.contains("commit_sha: 2222222222222222222222222222222222222222"));
	assert!(rendered.contains("closeout_status: Done"));
	assert!(rendered.contains("lifecycle_elapsed_seconds: 600"));
	assert!(rendered.contains("local_attempts: 2"));
	assert!(rendered.contains("lifecycle_bucket_breakdown"));
	assert!(
		rendered.contains(
			"lifecycle_bucket: Development lifecycle_bucket_key: development attempts: 2"
		)
	);
	assert!(!rendered.contains("pr_url: none"));
	assert_eq!(snapshot_json["history_lanes"][0]["latest_run"]["status"], "closeout");
	assert_eq!(snapshot_json["history_lanes"][0]["attempts"][0]["status"], "failed");
	assert_eq!(
		snapshot_json["recent_runs"].as_array().expect("recent runs should be an array").len(),
		0
	);
}
