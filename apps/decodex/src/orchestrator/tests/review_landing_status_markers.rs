use crate::{orchestrator::{self, tests, ReviewOrchestrationMarker, StateStore}, orchestrator::tests::{TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT, TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID, TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT}};

#[test]
fn ensure_review_orchestration_marker_ignores_stale_tracker_record_from_prior_handoff() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let current_pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let head_oid = "abc123";

	state_store
		.upsert_review_orchestration_marker(
			config.service_id(),
			&issue.id,
			&ReviewOrchestrationMarker::new(
				String::from("run-0"),
				7,
				String::from("x/pubfi-pub-101"),
				String::from("https://github.com/hack-ink/decodex/pull/99"),
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

	let marker = orchestrator::ensure_review_orchestration_marker(
		config.service_id(),
		&state_store,
		&issue,
		&tests::sample_review_handoff_marker("x/pubfi-pub-101", current_pr_url, head_oid),
		head_oid,
	)
	.expect("fresh review orchestration marker should initialize");

	assert_eq!(marker.run_id(), "run-1");
	assert_eq!(marker.attempt_number(), 1);
	assert_eq!(marker.pr_url(), current_pr_url);
	assert_eq!(marker.phase(), "request_pending");

	let persisted_marker = state_store
		.review_orchestration_marker(
			config.service_id(),
			&issue.id,
			&tests::sample_review_handoff_marker("x/pubfi-pub-101", current_pr_url, head_oid),
		)
		.expect("runtime orchestration lookup should succeed")
		.expect("runtime orchestration marker should persist");

	assert_eq!(persisted_marker.run_id(), "run-1");
	assert_eq!(persisted_marker.attempt_number(), 1);
	assert_eq!(persisted_marker.pr_url(), current_pr_url);
	assert_eq!(persisted_marker.phase(), "request_pending");
}
