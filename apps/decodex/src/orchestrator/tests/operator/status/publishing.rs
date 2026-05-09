#[test]
fn live_operator_status_snapshot_degrades_when_post_review_status_refresh_fails() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Review", &[]);
	let tracker = FakeTracker::with_refresh_error(vec![issue.clone()], "rate limited");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
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
	.expect("snapshot should degrade instead of failing");

	assert_eq!(snapshot.warnings, vec![String::from("post_review_lane_status_unavailable")]);
	assert_eq!(snapshot.worktrees.len(), 1);
	assert!(snapshot.post_review_lanes.is_empty());
}

#[test]
fn operator_state_snapshot_publish_skips_external_observers_after_tick_failure() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(vec![sample_issue("Todo", &[])]);
	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&["control_plane_tick_failed"],
		&[],
	)
	.expect("snapshot should build from local state");

	assert_eq!(
		snapshot.warnings,
		vec![
			String::from("control_plane_tick_failed"),
			String::from("external_observer_status_skipped"),
		]
	);
	assert_eq!(snapshot.projects[0].warning_count, 2);
	assert_eq!(snapshot.projects[0].connector_state, "degraded");
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"degraded publish should not query queued labels"
	);
}

#[test]
fn operator_state_snapshot_reports_tracker_rate_limit_as_backoff() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(vec![sample_issue("Todo", &[])]);
	let reset_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() + 60;
	let error = eyre::eyre!(
		"Linear connector is rate limited until `{reset_unix_epoch}`: API rate limit exceeded"
	);
	let connector_backoff = orchestrator::tracker_rate_limit_backoff(
		&error,
		Instant::now(),
		"operator_snapshot_refresh",
	)
	.expect("rate limit should create backoff")
	.to_operator_status(config.service_id(), reset_unix_epoch - 15);
	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[TRACKER_RATE_LIMIT_WARNING],
		slice::from_ref(&connector_backoff),
	)
	.expect("snapshot should build from local state");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(
		snapshot.warnings,
		vec![
			String::from(orchestrator::TRACKER_RATE_LIMIT_WARNING),
			String::from("external_observer_status_skipped"),
		]
	);
	assert_eq!(snapshot.connector_backoffs, vec![connector_backoff]);
	assert_eq!(snapshot.connector_backoffs[0].project_id, config.service_id());
	assert_eq!(snapshot.connector_backoffs[0].connector, "linear");
	assert_eq!(snapshot.connector_backoffs[0].sync_phase, "operator_snapshot_refresh");
	assert_eq!(snapshot.connector_backoffs[0].quota_class, "linear_graphql_api");
	assert_eq!(snapshot.connector_backoffs[0].reset_unix_epoch, reset_unix_epoch);
	assert_eq!(snapshot.connector_backoffs[0].reset_source, "linear");
	assert_eq!(snapshot.connector_backoffs[0].retry_after_seconds, 15);
	assert_eq!(snapshot.connector_backoffs[0].warning, orchestrator::TRACKER_RATE_LIMIT_WARNING);
	assert_eq!(snapshot_json["connector_backoffs"][0]["connector"], "linear");
	assert_eq!(
		snapshot_json["connector_backoffs"][0]["sync_phase"],
		"operator_snapshot_refresh"
	);
	assert_eq!(snapshot_json["connector_backoffs"][0]["reset_unix_epoch"], reset_unix_epoch);
	assert_eq!(snapshot_json["connector_backoffs"][0]["retry_after_seconds"], 15);
	assert_ne!(snapshot_json["connector_backoffs"][0]["reset_at"], Value::Null);
	assert_ne!(snapshot_json["connector_backoffs"][0]["next_action"], Value::Null);
	assert_eq!(snapshot.projects[0].connector_state, "backoff");
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"rate-limited publish should not query queued labels"
	);
}

#[test]
fn operator_state_snapshot_publish_does_not_derive_history_outcome_without_execution_ledger() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-1",
		"XY-355",
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
			"y/decodex-xy-355",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember project ownership");
	state_store
		.record_run_attempt("xy-355-attempt-1", &issue.id, 1, "succeeded")
		.expect("successful attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("completed lane cleanup should clear local worktree");

	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[],
		&[],
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(lane.ledger_outcome.ledger_status, "missing");
	assert_eq!(lane.ledger_outcome.final_outcome, "execution_ledger_missing");
	assert_eq!(lane.ledger_outcome.record_count, 0);
	assert_eq!(
		lane.ledger_outcome.summary.as_deref(),
		Some("No decodex.linear_execution_event records are available for this history lane.")
	);
	assert_eq!(
		snapshot_json["history_lanes"][0]["ledger_outcome"]["ledger_status"],
		"missing"
	);
	assert_eq!(
		snapshot_json["history_lanes"][0]["ledger_outcome"]["final_outcome"],
		"execution_ledger_missing"
	);
	assert_ne!(
		snapshot_json["history_lanes"][0]["ledger_outcome"]["ledger_status"],
		Value::Null
	);
	assert_ne!(
		snapshot_json["history_lanes"][0]["ledger_outcome"]["final_outcome"],
		Value::Null
	);
	assert!(
		tracker.comment_queries.borrow().is_empty(),
		"control-plane publish should not replay Linear history comments every tick"
	);
}

#[test]
fn operator_state_snapshot_publish_reads_local_completed_ledger_details_without_comment_replay() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-1",
		"XY-355",
		"Done",
		&[],
		Some(3),
		"2026-04-29T10:11:00Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let local_comments = successful_linear_execution_history_comments_with_cleanup(&issue);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/decodex-xy-355",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember project ownership");
	state_store
		.record_run_attempt("xy-355-attempt-1", &issue.id, 1, "succeeded")
		.expect("successful attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("completed lane cleanup should clear local worktree");

	seed_local_linear_execution_events(&state_store, &local_comments);

	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[],
		&[],
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(lane.ledger_outcome.ledger_status, "present");
	assert_eq!(lane.ledger_outcome.final_outcome, "cleanup_complete");
	assert_eq!(
		lane.ledger_outcome.pr_url.as_deref(),
		Some("https://github.com/hack-ink/decodex/pull/355")
	);
	assert_eq!(
		lane.ledger_outcome.commit_sha.as_deref(),
		Some("2222222222222222222222222222222222222222")
	);
	assert_eq!(lane.ledger_outcome.closeout_status.as_deref(), Some("completed"));
	assert_eq!(lane.ledger_outcome.lifecycle_elapsed_seconds, Some(660));
	assert_eq!(lane.ledger_outcome.record_count, 6);
	assert_eq!(
		snapshot_json["history_lanes"][0]["ledger_outcome"]["ledger_status"],
		"present"
	);
	assert_eq!(
		snapshot_json["history_lanes"][0]["ledger_outcome"]["pr_url"],
		"https://github.com/hack-ink/decodex/pull/355"
	);
	assert!(
		tracker.comment_queries.borrow().is_empty(),
		"control-plane publish should use local execution events instead of replaying Linear comments"
	);
}

#[test]
fn tracker_rate_limit_error_enters_control_plane_backoff() {
	let now = Instant::now();
	let error = eyre::eyre!(
		"Linear connector is rate limited: Rate limit exceeded. Only 2500 requests are allowed per 1 hour."
	);
	let backoff_until = orchestrator::tracker_rate_limit_backoff(&error, now, "control_plane_tick")
		.expect("rate limit should create backoff");

	assert!(backoff_until.until > now);
}

#[test]
fn tracker_rate_limit_error_uses_reset_timestamp_when_available() {
	let now = Instant::now();
	let reset_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() + 30;
	let error = eyre::eyre!(
		"Linear connector is rate limited until `{reset_unix_epoch}`: API rate limit exceeded"
	);
	let backoff_until = orchestrator::tracker_rate_limit_backoff(&error, now, "control_plane_tick")
		.expect("rate limit reset should create backoff");

	assert!(backoff_until.until >= now + Duration::from_secs(29));
	assert!(backoff_until.until <= now + Duration::from_secs(31));
	assert_eq!(backoff_until.reset_unix_epoch, reset_unix_epoch);
	assert_eq!(backoff_until.reset_source, "linear");
	assert_eq!(backoff_until.sync_phase, "control_plane_tick");
}
