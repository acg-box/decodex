#[test]
fn operator_status_history_limit_applies_after_active_runs_are_split_out() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_issue = sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let failed_issue = sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	state_store
		.record_run_attempt("run-active", &active_issue.id, 1, "running")
		.expect("active run should record");
	state_store
		.upsert_lease("pubfi", &active_issue.id, "run-active", "In Progress")
		.expect("active lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&active_issue.id,
			"x/pubfi-pub-101",
			&config.worktree_root().join(&active_issue.identifier).display().to_string(),
		)
		.expect("active worktree should record");
	state_store
		.record_run_attempt("run-failed", &failed_issue.id, 1, "failed")
		.expect("failed run should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&failed_issue.id,
			"x/pubfi-pub-102",
			&config.worktree_root().join(&failed_issue.identifier).display().to_string(),
		)
		.expect("failed worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 1)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.run_limit, 1);
	assert_eq!(snapshot.active_runs.len(), 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert_eq!(snapshot.history_lanes[0].attempt_count, 1);
	assert!(rendered.contains(
		"Run ledger shown: 1 issue lanes from 1 history attempts (running lanes inline)"
	));
	assert_eq!(rendered.matches("run_id: run-active").count(), 1);
	assert_eq!(rendered.matches("run_id: run-failed").count(), 1);

	let history_index = rendered.find("Run Ledger").expect("history section should render");
	let failed_index = rendered.find("run_id: run-failed").expect("failed run should render");

	assert!(
		failed_index > history_index,
		"non-running history run should remain visible after running lane overlap is hidden"
	);
}

#[test]
fn operator_status_history_lanes_group_attempts_by_issue() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue = sample_issue_with_sort_fields(
		"issue-1",
		"XY-323",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_issue = sample_issue_with_sort_fields(
		"issue-2",
		"XY-330",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	state_store
		.record_run_attempt("xy-323-attempt-1-1777361523", &first_issue.id, 1, "failed")
		.expect("first attempt should record");
	state_store
		.record_run_attempt("xy-323-attempt-2-1777361550", &first_issue.id, 2, "succeeded")
		.expect("second attempt should record");
	state_store
		.record_run_attempt("xy-330-attempt-1-1777361600", &second_issue.id, 1, "succeeded")
		.expect("other issue attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&first_issue.id,
			"x/decodex-xy-323",
			&config.worktree_root().join(&first_issue.identifier).display().to_string(),
		)
		.expect("first issue worktree should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&second_issue.id,
			"x/decodex-xy-330",
			&config.worktree_root().join(&second_issue.identifier).display().to_string(),
		)
		.expect("second issue worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let grouped_lane = snapshot
		.history_lanes
		.iter()
		.find(|lane| lane.issue_key == "XY-323")
		.expect("first issue should have a grouped history lane");

	assert_eq!(snapshot.recent_runs.len(), 3);
	assert_eq!(snapshot.history_lanes.len(), 2);
	assert_eq!(grouped_lane.attempt_count, 2);
	assert_eq!(grouped_lane.latest_run.run_id, "xy-323-attempt-2-1777361550");
	assert!(rendered.contains("Run ledger shown: 2 issue lanes from 3 history attempts"));
	assert!(rendered.contains("issue: XY-323"));
	assert!(rendered.contains("attempts: 2"));
	assert!(rendered.contains("attempt_timeline"));
}

#[test]
fn operator_status_project_waiting_count_ignores_superseded_waiting_attempts() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-1",
		"XY-451",
		"Done",
		&[],
		Some(3),
		"2026-05-03T11:48:16Z",
	);

	state_store
		.record_run_attempt("xy-451-attempt-1-1777791228", &issue.id, 1, "stalled")
		.expect("stalled attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/decodex-xy-451",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.record_run_attempt("xy-451-attempt-4-1777808209", &issue.id, 4, "succeeded")
		.expect("successful attempt should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let grouped_lane = snapshot.history_lanes.first().expect("history lane should exist");

	assert_eq!(grouped_lane.attempt_count, 2);
	assert_eq!(grouped_lane.latest_run.run_id, "xy-451-attempt-4-1777808209");
	assert_eq!(snapshot.projects[0].waiting_lane_count, 0);
}

#[test]
fn operator_status_project_connector_state_ignores_superseded_retry_backoff_attempts() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-1",
		"XY-452",
		"Done",
		&[],
		Some(3),
		"2026-05-03T11:49:16Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.record_run_attempt("xy-452-attempt-1-1777791228", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/decodex-xy-452",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_retry_schedule(
		&worktree_path,
		"xy-452-attempt-1-1777791228",
		1,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("retry schedule marker should write");

	state_store
		.record_run_attempt("xy-452-attempt-2-1777808209", &issue.id, 2, "succeeded")
		.expect("successful attempt should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let grouped_lane = snapshot.history_lanes.first().expect("history lane should exist");

	assert_eq!(grouped_lane.latest_run.run_id, "xy-452-attempt-2-1777808209");
	assert_eq!(snapshot.projects[0].waiting_lane_count, 0);
	assert_eq!(snapshot.projects[0].connector_state, "ok");
}

#[test]
fn live_operator_history_lanes_prefer_linear_ledger_outcome() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = sample_issue_with_sort_fields(
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
		.record_run_attempt("xy-355-attempt-2-1777527613", &issue.id, 2, "succeeded")
		.expect("successful attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("completed lane cleanup should clear local worktree");
	tracker
		.issue_comments
		.borrow_mut()
		.insert(issue.id.clone(), successful_linear_execution_history_comments(&issue));

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
	let outcome_index = rendered.find("outcome: closeout").expect("ledger outcome should render");
	let local_index = rendered.find("latest_run_id:").expect("local attempt debug should render");

	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert!(snapshot.recent_runs.iter().all(|run| run.project_id == TEST_SERVICE_ID));
	assert!(snapshot.recent_runs.iter().all(|run| {
		run.issue_identifier.as_deref() == Some("XY-355")
			&& run.title.as_deref() == Some("Keep completed run rows self describing")
	}));
	assert_eq!(lane.project_id, TEST_SERVICE_ID);
	assert_eq!(lane.issue_identifier.as_deref(), Some("XY-355"));
	assert_eq!(lane.title.as_deref(), Some("Keep completed run rows self describing"));
	assert_eq!(lane.latest_run.issue_identifier.as_deref(), Some("XY-355"));
	assert_eq!(lane.latest_run.title.as_deref(), Some("Keep completed run rows self describing"));
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
	assert!(!rendered.contains("pr_url: none"));
}

#[test]
fn live_operator_history_lanes_require_linear_execution_ledger_records() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-1",
		"XY-356",
		"Done",
		&[],
		Some(3),
		"2026-04-29T10:11:00Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/decodex-xy-356",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember project ownership");
	state_store
		.record_run_attempt("xy-356-attempt-1", &issue.id, 1, "succeeded")
		.expect("successful attempt should record");
	state_store.clear_worktree(&issue.id).expect("completed lane cleanup should clear local worktree");

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

	assert_eq!(tracker.comment_queries.borrow().as_slice(), slice::from_ref(&issue.id));
	assert_eq!(lane.ledger_outcome.ledger_status, "missing");
	assert_eq!(lane.ledger_outcome.final_outcome, "execution_ledger_missing");
	assert_eq!(lane.ledger_outcome.record_count, 0);
	assert_eq!(
		lane.ledger_outcome.summary.as_deref(),
		Some("No decodex.linear_execution_event records are available for this history lane.")
	);
	assert!(rendered.contains("ledger_status: missing"));
	assert!(rendered.contains("outcome: execution_ledger_missing"));
}
