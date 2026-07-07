use crate::orchestrator::{
	self, ReviewLifecycleTransitionFixture, StateStore,
	tests::{
		self, TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT, TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT,
	},
};

#[test]
fn ensure_review_lifecycle_authority_ignores_stale_marker_projection_from_prior_handoff() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let current_pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let head_oid = "abc123";
	let branch_name = "x/pubfi-pub-101";
	let stale_pr_url = "https://github.com/hack-ink/decodex/pull/99";

	let stale_handoff =
		tests::sample_review_lifecycle_handoff_fixture(branch_name, stale_pr_url, "deadbeef");
	state_store
		.upsert_review_lifecycle_handoff_fixture(config.service_id(), &issue.id, &stale_handoff)
		.expect("stale handoff should persist");
	state_store
		.upsert_review_lifecycle_transition_fixture(
			config.service_id(),
			&issue.id,
			&ReviewLifecycleTransitionFixture::new(
				String::from("run-0"),
				7,
				String::from(branch_name),
				String::from(stale_pr_url),
				String::from("deadbeef"),
				String::from("waiting_for_merge"),
				Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID),
				Some(TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT),
				Some(1),
				2,
				3,
				Some(TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT),
			),
		)
		.expect("stale orchestration marker should persist");

	let current_handoff =
		tests::sample_review_lifecycle_handoff_fixture(branch_name, current_pr_url, head_oid);
	state_store
		.upsert_review_lifecycle_handoff_fixture(config.service_id(), &issue.id, &current_handoff)
		.expect("current lifecycle authority should persist");
	let lifecycle_record = state_store
		.review_lifecycle_record(config.service_id(), &issue.id, branch_name)
		.expect("lifecycle lookup should succeed")
		.expect("current lifecycle authority should exist");
	let record = orchestrator::ensure_review_lifecycle_authority(
		config.service_id(),
		&state_store,
		&issue,
		&lifecycle_record,
		head_oid,
	)
	.expect("fresh review lifecycle authority should initialize");

	assert_eq!(record.run_id(), "run-1");
	assert_eq!(record.attempt_number(), 1);
	assert_eq!(record.pr_url(), current_pr_url);
	assert_eq!(record.phase(), "request_pending");

	let persisted_record = state_store
		.review_lifecycle_record(config.service_id(), &issue.id, branch_name)
		.expect("lifecycle lookup should succeed")
		.expect("runtime lifecycle authority should persist");

	assert_eq!(persisted_record.run_id(), "run-1");
	assert_eq!(persisted_record.attempt_number(), 1);
	assert_eq!(persisted_record.pr_url(), current_pr_url);
	assert_eq!(persisted_record.phase(), "request_pending");
}
