use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, ReviewLifecycleHandoffFixture, StateStore, TEST_SERVICE_ID, orchestrator,
};

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
	let handoff = ReviewLifecycleHandoffFixture::new(
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
		.upsert_review_lifecycle_handoff_fixture(TEST_SERVICE_ID, &issue.id, &handoff)
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
