use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, Instant, OffsetDateTime, ReviewHandoffMarker, StateStore, TEST_SERVICE_ID,
	TRACKER_RATE_LIMIT_WARNING, TRACKER_TRANSIENT_TIMEOUT_WARNING, Value, eyre, orchestrator,
	slice,
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
