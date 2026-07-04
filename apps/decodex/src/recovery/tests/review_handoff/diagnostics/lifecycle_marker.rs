use crate::{
	prelude::{Result, eyre},
	recovery::tests::review_handoff,
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore},
};

#[test]
fn rebind_lifecycle_marker_write_failure_clears_partial_handoff_marker() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let error = review_handoff::write_review_lifecycle_markers_with_rollback(
		&state_store,
		"pubfi",
		"issue-id",
		&handoff,
		&orchestration,
		|| -> Result<()> { Err(eyre::eyre!("orchestration marker write failed")) },
	)
	.expect_err("orchestration write failure should be returned");

	assert!(error.to_string().contains("orchestration marker write failed"));
	assert!(
		state_store
			.review_lifecycle_record("pubfi", "issue-id", branch_name)
			.expect("lifecycle read should succeed")
			.is_none()
	);
	assert!(
		state_store
			.review_handoff_marker("pubfi", "issue-id", branch_name)
			.expect("handoff read should succeed")
			.is_none()
	);
}
