use crate::state::{ReviewLifecycleHandoffFixture, ReviewLifecycleTransitionFixture, StateStore};

#[test]
fn changed_review_handoff_authority_resets_lifecycle_phase_fields() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let old_handoff = ReviewLifecycleHandoffFixture::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let old_orchestration = ReviewLifecycleTransitionFixture::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"19b20f7dfb9526e7421a5f095b1c6adec84e52d7",
		"waiting_for_ack",
		Some(1_234),
		Some(1_775_200_000),
		Some(3),
		2,
		4,
		Some(1_775_200_900),
	);
	let new_handoff = ReviewLifecycleHandoffFixture::new(
		"run-2",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"28c20f7dfb9526e7421a5f095b1c6adec84e52d8",
	);

	store
		.upsert_review_lifecycle_handoff_fixture("pubfi", "PUB-101", &old_handoff)
		.expect("old handoff projection should persist");
	store
		.upsert_review_lifecycle_transition_fixture("pubfi", "PUB-101", &old_orchestration)
		.expect("old orchestration projection should persist");
	store
		.upsert_review_lifecycle_handoff_fixture("pubfi", "PUB-101", &new_handoff)
		.expect("changed handoff projection should persist");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review lifecycle record should read")
		.expect("review lifecycle record should exist");

	assert_eq!(lifecycle.run_id(), "run-2");
	assert_eq!(lifecycle.pr_head_oid(), "28c20f7dfb9526e7421a5f095b1c6adec84e52d8");
	assert_eq!(lifecycle.head_sha(), "28c20f7dfb9526e7421a5f095b1c6adec84e52d8");
	assert_eq!(lifecycle.phase(), "request_pending");
	assert_eq!(lifecycle.request_comment_database_id(), None);
	assert_eq!(lifecycle.request_created_at_unix_epoch(), None);
	assert_eq!(lifecycle.request_description_thumbs_up_count(), None);
	assert_eq!(lifecycle.request_retry_count(), 0);
	assert_eq!(lifecycle.external_round_count(), 0);
	assert_eq!(lifecycle.auto_merge_enabled_at_unix_epoch(), None);
	assert_eq!(lifecycle.landing_state(), "not_started");
	assert_eq!(lifecycle.closeout_state(), "not_started");
	assert_eq!(lifecycle.repair_attempt_count(), 0);
	assert!(lifecycle.evidence_json().contains("lifecycle_authority_recorded"));
	assert_eq!(lifecycle.transition(), "review_handoff_recorded");
	assert_eq!(lifecycle.previous_state(), "review_waiting");
	assert_eq!(lifecycle.next_state(), "review_pending");
	assert_eq!(lifecycle.next_action(), "wait_for_runtime_review_gate_or_external_review");

	let orchestration = store
		.review_lifecycle_transition_fixture("pubfi", "PUB-101", &new_handoff)
		.expect("new orchestration projection should read")
		.expect("new orchestration projection should exist");

	assert_eq!(orchestration.run_id(), "run-2");
	assert_eq!(orchestration.head_sha(), "28c20f7dfb9526e7421a5f095b1c6adec84e52d8");
	assert_eq!(orchestration.phase(), "request_pending");
	assert_eq!(orchestration.request_retry_count(), 0);
	assert_eq!(orchestration.external_round_count(), 0);
}
