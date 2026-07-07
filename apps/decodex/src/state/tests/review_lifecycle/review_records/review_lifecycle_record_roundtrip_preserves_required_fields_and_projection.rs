use crate::state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore};

#[test]
fn review_lifecycle_record_roundtrip_preserves_required_fields_and_projection() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("review handoff projection should persist");

	let restored_handoff = store
		.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review handoff projection should read")
		.expect("review handoff projection should exist");

	assert_eq!(restored_handoff, handoff);

	let orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"waiting_for_ack",
		Some(1_234),
		Some(1_775_200_000),
		Some(3),
		1,
		2,
		Some(1_775_200_900),
	);

	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("review orchestration projection should persist");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review lifecycle record should read")
		.expect("review lifecycle record should exist");

	assert_eq!(lifecycle.project_id(), "pubfi");
	assert_eq!(lifecycle.issue_id(), "PUB-101");
	assert_eq!(lifecycle.branch_name(), "x/decodex-pub-101");
	assert_eq!(lifecycle.run_id(), "run-1");
	assert_eq!(lifecycle.attempt_number(), 2);
	assert_eq!(lifecycle.pr_url(), "https://github.com/hack-ink/decodex/pull/101");
	assert_eq!(lifecycle.target_base_ref_name(), Some("main"));
	assert_eq!(lifecycle.pr_head_ref_name(), "x/decodex-pub-101");
	assert_eq!(lifecycle.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(lifecycle.head_sha(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(lifecycle.phase(), "waiting_for_ack");
	assert_eq!(lifecycle.request_comment_database_id(), Some(1_234));
	assert_eq!(lifecycle.request_created_at_unix_epoch(), Some(1_775_200_000));
	assert_eq!(lifecycle.request_description_thumbs_up_count(), Some(3));
	assert_eq!(lifecycle.request_retry_count(), 1);
	assert_eq!(lifecycle.external_round_count(), 2);
	assert_eq!(lifecycle.auto_merge_enabled_at_unix_epoch(), Some(1_775_200_900));
	assert_eq!(lifecycle.landing_state(), "not_started");
	assert_eq!(lifecycle.closeout_state(), "not_started");
	assert_eq!(lifecycle.repair_attempt_count(), 0);
	assert!(lifecycle.evidence_json().contains("lifecycle_authority_recorded"));
	assert_eq!(lifecycle.sequence(), 2);
	assert_eq!(lifecycle.transition(), "review_wait_recorded");
	assert_eq!(lifecycle.previous_state(), "review_pending");
	assert_eq!(lifecycle.next_state(), "review_waiting");
	assert_eq!(lifecycle.next_action(), "wait_for_external_review_ack");
	assert!(!lifecycle.updated_at().is_empty());
	assert!(lifecycle.updated_at_unix() > 0);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("same handoff projection should persist without resetting lifecycle state");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review lifecycle record should read after same handoff")
		.expect("review lifecycle record should exist after same handoff");

	assert_eq!(lifecycle.phase(), "waiting_for_ack");
	assert_eq!(lifecycle.request_comment_database_id(), Some(1_234));

	let restored_orchestration = store
		.review_orchestration_marker("pubfi", "PUB-101", &handoff)
		.expect("review orchestration projection should read")
		.expect("review orchestration projection should exist");

	assert_eq!(restored_orchestration, orchestration);

	let snapshot = store
		.project_loop_evidence_snapshot("pubfi")
		.expect("project loop evidence snapshot should read");
	let snapshot_lifecycle = snapshot
		.review_lifecycle_record("PUB-101", "x/decodex-pub-101")
		.expect("snapshot review lifecycle should exist");

	assert_eq!(snapshot_lifecycle, &lifecycle);
}
