use crate::{
	orchestrator::tests::operator::{
		status,
		status::{
			Duration, FakeTracker, Instant, OffsetDateTime, ReviewHandoffMarker, StateStore,
			TEST_SERVICE_ID, TRACKER_RATE_LIMIT_WARNING, TRACKER_TRANSIENT_TIMEOUT_WARNING, Value,
			eyre, orchestrator, slice,
		},
	},
	state::ConnectorBackoffInput,
};

#[test]
fn live_operator_status_snapshot_degrades_when_post_review_status_refresh_fails() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue("In Review", &[]);
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
fn live_operator_status_snapshot_preserves_retained_handoff_during_linear_backoff() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue("In Review", &[]);
	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear connector timed out during GraphQL request: deadline elapsed",
	);
	let branch_name = "x/pubfi-pub-101";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/101";
	let head_sha = "1111111111111111111111111111111111111111";
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let handoff = ReviewHandoffMarker::new(
		"pub-101-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_sha,
	);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			branch_name,
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.upsert_review_handoff_marker(TEST_SERVICE_ID, &issue.id, &handoff)
		.expect("review handoff should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should degrade instead of failing");

	assert!(
		snapshot.warnings.contains(&String::from(orchestrator::TRACKER_TRANSIENT_TIMEOUT_WARNING))
	);
	assert!(snapshot.warnings.contains(&String::from("external_observer_status_skipped")));
	assert_eq!(snapshot.connector_backoffs.len(), 1);
	assert_eq!(snapshot.connector_backoffs[0].sync_phase, "post_review_lane_status");
	assert_eq!(snapshot.connector_backoffs[0].quota_class, "linear_graphql_timeout");
	assert_eq!(
		snapshot.connector_backoffs[0].warning,
		orchestrator::TRACKER_TRANSIENT_TIMEOUT_WARNING
	);
	assert_eq!(snapshot.post_review_lanes.len(), 1);
	assert_eq!(snapshot.post_review_lanes[0].issue_identifier, "PUB-101");
	assert_eq!(snapshot.post_review_lanes[0].issue_state, "tracker_readback_degraded");
	assert_eq!(snapshot.post_review_lanes[0].reason, "tracker_issue_readback_degraded");
	assert_eq!(snapshot.post_review_lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(snapshot.post_review_lanes[0].pr_head_sha.as_deref(), Some(head_sha));
	assert_eq!(
		snapshot.post_review_lanes[0].readback_warning.as_deref(),
		Some("tracker_issue_readback_degraded")
	);
	assert_eq!(
		snapshot.post_review_lanes[0].readback_root_cause.as_deref(),
		Some("tracker_issue_readback_failed")
	);
	assert!(
		state_store
			.connector_backoff(TEST_SERVICE_ID, "linear")
			.expect("connector backoff should read")
			.is_some()
	);
}

#[test]
fn operator_state_snapshot_publish_skips_external_observers_after_tick_failure() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(vec![status::sample_issue("Todo", &[])]);
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
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let branch_name = "x/pubfi-pub-101";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/101";
	let head_sha = "1111111111111111111111111111111111111111";
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let reset_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() + 60;
	let error = eyre::eyre!(
		"Linear connector is rate limited until `{reset_unix_epoch}`: API rate limit exceeded"
	);
	let connector_backoff = orchestrator::tracker_connector_backoff(
		&error,
		Instant::now(),
		"operator_snapshot_refresh",
	)
	.expect("rate limit should create backoff")
	.to_operator_status(config.service_id(), reset_unix_epoch - 15);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			branch_name,
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.upsert_review_handoff_marker(
			TEST_SERVICE_ID,
			&issue.id,
			&ReviewHandoffMarker::new(
				"pub-101-attempt-1",
				1,
				branch_name,
				pr_url,
				"main",
				branch_name,
				head_sha,
			),
		)
		.expect("review handoff should record");

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
	assert_eq!(snapshot.connector_backoffs[0].quota_class, "linear_graphql_rate_limit");
	assert_eq!(snapshot.connector_backoffs[0].reset_unix_epoch, reset_unix_epoch);
	assert_eq!(snapshot.connector_backoffs[0].reset_source, "linear");
	assert_eq!(snapshot.connector_backoffs[0].retry_after_seconds, 15);
	assert_eq!(snapshot.connector_backoffs[0].warning, orchestrator::TRACKER_RATE_LIMIT_WARNING);
	assert_eq!(snapshot_json["connector_backoffs"][0]["connector"], "linear");
	assert_eq!(snapshot_json["connector_backoffs"][0]["sync_phase"], "operator_snapshot_refresh");
	assert_eq!(snapshot_json["connector_backoffs"][0]["reset_unix_epoch"], reset_unix_epoch);
	assert_eq!(snapshot_json["connector_backoffs"][0]["retry_after_seconds"], 15);
	assert_ne!(snapshot_json["connector_backoffs"][0]["reset_at"], Value::Null);
	assert_ne!(snapshot_json["connector_backoffs"][0]["next_action"], Value::Null);
	assert_eq!(snapshot.post_review_lanes.len(), 1);
	assert_eq!(snapshot.post_review_lanes[0].issue_identifier, "PUB-101");
	assert_eq!(snapshot.post_review_lanes[0].reason, "tracker_issue_readback_degraded");
	assert_eq!(snapshot.post_review_lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(snapshot.post_review_lanes[0].pr_head_sha.as_deref(), Some(head_sha));
	assert_eq!(
		snapshot.post_review_lanes[0].readback_root_cause.as_deref(),
		Some("tracker_issue_readback_failed")
	);
	assert_eq!(
		snapshot_json["post_review_lanes"][0]["readback_root_cause"],
		"tracker_issue_readback_failed"
	);
	assert_eq!(snapshot.projects[0].connector_state, "backoff");
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"rate-limited publish should not query queued labels"
	);
}

#[test]
fn operator_state_snapshot_reports_tracker_timeout_as_transient_backoff() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(vec![status::sample_issue("Todo", &[])]);
	let now = Instant::now();
	let error = eyre::eyre!("Linear connector timed out during GraphQL request: deadline elapsed");
	let connector_backoff =
		orchestrator::tracker_connector_backoff(&error, now, "operator_snapshot_refresh")
			.expect("timeout should create transient backoff")
			.to_operator_status(config.service_id(), OffsetDateTime::now_utc().unix_timestamp());
	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[TRACKER_TRANSIENT_TIMEOUT_WARNING],
		slice::from_ref(&connector_backoff),
	)
	.expect("snapshot should build from local state");

	assert_eq!(
		snapshot.warnings,
		vec![
			String::from(orchestrator::TRACKER_TRANSIENT_TIMEOUT_WARNING),
			String::from("external_observer_status_skipped"),
		]
	);
	assert_eq!(snapshot.connector_backoffs, vec![connector_backoff]);
	assert_eq!(snapshot.connector_backoffs[0].connector, "linear");
	assert_eq!(snapshot.connector_backoffs[0].sync_phase, "operator_snapshot_refresh");
	assert_eq!(snapshot.connector_backoffs[0].quota_class, "linear_graphql_timeout");
	assert_eq!(snapshot.connector_backoffs[0].reset_source, "local_transient_timeout");
	assert_eq!(
		snapshot.connector_backoffs[0].warning,
		orchestrator::TRACKER_TRANSIENT_TIMEOUT_WARNING
	);
	assert_eq!(snapshot.projects[0].connector_state, "backoff");
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"timeout publish should not query queued labels during backoff"
	);
}

#[test]
fn live_operator_status_snapshot_honors_persisted_tracker_backoff_without_linear_reads() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(vec![status::sample_issue("Todo", &[])]);
	let reset_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() + 60;

	state_store
		.upsert_connector_backoff(ConnectorBackoffInput {
			project_id: config.service_id(),
			connector: "linear",
			sync_phase: "run_cycle",
			quota_class: "linear_graphql_rate_limit",
			reset_unix_epoch,
			reset_source: "local_default",
			warning: TRACKER_RATE_LIMIT_WARNING,
		})
		.expect("connector backoff should persist");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should use local backoff state");

	assert!(snapshot.warnings.contains(&String::from(orchestrator::TRACKER_RATE_LIMIT_WARNING)));
	assert!(snapshot.warnings.contains(&String::from("external_observer_status_skipped")));
	assert_eq!(snapshot.connector_backoffs.len(), 1);
	assert_eq!(snapshot.projects[0].connector_state, "backoff");
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"persisted backoff should skip queued-label reads"
	);
	assert!(
		tracker.comment_queries.borrow().is_empty(),
		"persisted backoff should skip execution-ledger reads"
	);
}

#[test]
fn operator_state_snapshot_publish_does_not_derive_history_outcome_without_execution_ledger() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
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
	assert_eq!(snapshot_json["history_lanes"][0]["ledger_outcome"]["ledger_status"], "missing");
	assert_eq!(
		snapshot_json["history_lanes"][0]["ledger_outcome"]["final_outcome"],
		"execution_ledger_missing"
	);
	assert_ne!(snapshot_json["history_lanes"][0]["ledger_outcome"]["ledger_status"], Value::Null);
	assert_ne!(snapshot_json["history_lanes"][0]["ledger_outcome"]["final_outcome"], Value::Null);
	assert!(
		tracker.comment_queries.borrow().is_empty(),
		"control-plane publish should not replay Linear history comments every tick"
	);
}

#[test]
fn operator_state_snapshot_publish_skips_terminal_run_metadata_refresh() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-355",
		"Done",
		&[],
		Some(3),
		"2026-04-29T10:11:00Z",
	);
	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear connector is rate limited: Rate limit exceeded. Only 2500 requests are allowed per 1 hour.",
	);

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
	.expect("terminal-only publish should avoid Linear metadata refresh");

	assert_eq!(snapshot.history_lanes.len(), 1);
	assert!(
		!snapshot
			.warnings
			.iter()
			.any(|warning| warning == orchestrator::TRACKER_RATE_LIMIT_WARNING),
		"terminal-only publish should not enter backoff from run metadata"
	);
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"control-plane publish should not refresh terminal recent/history run metadata"
	);
}

#[test]
fn operator_state_snapshot_publish_still_refreshes_current_lane_metadata() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-355",
		"In Progress",
		&[],
		Some(3),
		"2026-04-29T10:11:00Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.record_run_attempt("xy-355-attempt-1", &issue.id, 1, "running")
		.expect("running attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, "xy-355-attempt-1", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/decodex-xy-355",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should remember project ownership");

	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[],
		&[],
	)
	.expect("current-lane publish should build");
	let refresh_queries = tracker.refresh_queries.borrow();

	assert!(
		refresh_queries.iter().any(|query| query.len() == 1 && query.first() == Some(&issue.id)),
		"current-lane publish should still refresh the current lane issue metadata"
	);
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].issue_identifier.as_deref(), Some("XY-355"));
	assert_eq!(snapshot.current_lanes[0].title.as_deref(), Some("Implement orchestration"));
	assert_eq!(snapshot.current_lanes[0].author.as_deref(), Some("Yvette"));

	let snapshot_json =
		orchestrator::operator_snapshot_json_value(&snapshot).expect("snapshot should project");

	assert_eq!(snapshot_json["presentation"]["schema"], "decodex.operator.presentation/1");
	assert_eq!(
		snapshot_json["presentation"]["current_lane_cards"][0]["run_id"],
		"xy-355-attempt-1"
	);
	assert_eq!(snapshot_json["presentation"]["current_lane_cards"][0]["title"], "XY-355");
	assert_eq!(
		snapshot_json["presentation"]["current_lane_cards"][0]["run"]["title"],
		"Implement orchestration"
	);
}

#[test]
fn operator_state_snapshot_publish_reads_local_completed_ledger_details_without_comment_replay() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-355",
		"Done",
		&[],
		Some(3),
		"2026-04-29T10:11:00Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let local_comments = status::successful_linear_execution_history_comments_with_cleanup(&issue);

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

	status::seed_local_linear_execution_events(&state_store, &local_comments);

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
	assert_eq!(snapshot_json["history_lanes"][0]["ledger_outcome"]["ledger_status"], "present");
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
	let backoff_until = orchestrator::tracker_connector_backoff(&error, now, "control_plane_tick")
		.expect("rate limit should create backoff");

	assert!(backoff_until.until > now);
}

#[test]
fn tracker_timeout_error_enters_transient_control_plane_backoff() {
	let now = Instant::now();
	let error = eyre::eyre!("Linear connector timed out during GraphQL request: deadline elapsed");
	let backoff = orchestrator::tracker_connector_backoff(&error, now, "control_plane_tick")
		.expect("timeout should create transient tracker backoff");

	assert!(backoff.until >= now + Duration::from_secs(59));
	assert!(backoff.until <= now + Duration::from_secs(61));
	assert_eq!(backoff.quota_class, "linear_graphql_timeout");
	assert_eq!(backoff.reset_source, "local_transient_timeout");
	assert_eq!(backoff.sync_phase, "control_plane_tick");
	assert_eq!(backoff.warning, orchestrator::TRACKER_TRANSIENT_TIMEOUT_WARNING);
}

#[test]
fn tracker_rate_limit_error_uses_reset_timestamp_when_available() {
	let now = Instant::now();
	let reset_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() + 30;
	let error = eyre::eyre!(
		"Linear connector is rate limited until `{reset_unix_epoch}`: API rate limit exceeded"
	);
	let backoff_until = orchestrator::tracker_connector_backoff(&error, now, "control_plane_tick")
		.expect("rate limit reset should create backoff");

	assert!(backoff_until.until >= now + Duration::from_secs(29));
	assert!(backoff_until.until <= now + Duration::from_secs(31));
	assert_eq!(backoff_until.quota_class, "linear_graphql_rate_limit");
	assert_eq!(backoff_until.reset_unix_epoch, reset_unix_epoch);
	assert_eq!(backoff_until.reset_source, "linear");
	assert_eq!(backoff_until.sync_phase, "control_plane_tick");
	assert_eq!(backoff_until.warning, orchestrator::TRACKER_RATE_LIMIT_WARNING);
}
