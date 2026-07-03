use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore, tests};

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
	assert_eq!(lifecycle.evidence_json(), "{}");
	assert_eq!(lifecycle.next_action(), "");
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

#[test]
fn changed_review_handoff_projection_resets_lifecycle_phase_fields() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let old_handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let old_orchestration = ReviewOrchestrationMarker::new(
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
	let new_handoff = ReviewHandoffMarker::new(
		"run-2",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"28c20f7dfb9526e7421a5f095b1c6adec84e52d8",
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &old_handoff)
		.expect("old handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &old_orchestration)
		.expect("old orchestration projection should persist");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &new_handoff)
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
	assert_eq!(lifecycle.evidence_json(), "{}");
	assert_eq!(lifecycle.next_action(), "");

	let orchestration = store
		.review_orchestration_marker("pubfi", "PUB-101", &new_handoff)
		.expect("new orchestration projection should read")
		.expect("new orchestration projection should exist");

	assert_eq!(orchestration.run_id(), "run-2");
	assert_eq!(orchestration.head_sha(), "28c20f7dfb9526e7421a5f095b1c6adec84e52d8");
	assert_eq!(orchestration.phase(), "request_pending");
	assert_eq!(orchestration.request_retry_count(), 0);
	assert_eq!(orchestration.external_round_count(), 0);
}

#[test]
fn historical_review_marker_tables_drop_without_lifecycle_migration() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");

	tests::seed_dropped_review_marker_tables(&state_path);

	let store = StateStore::open(&state_path).expect("state store should drop historical markers");

	assert!(
		store
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff projection should read")
			.is_none(),
		"historical review_handoffs rows must not become lifecycle records"
	);
	assert!(
		store
			.review_lifecycle_record("pubfi", "PUB-202", "x/decodex-pub-202")
			.expect("orchestration-only lifecycle should read")
			.is_none(),
		"historical review_orchestrations rows must not become lifecycle records"
	);
	assert!(
		store
			.review_lifecycle_record("pubfi", "PUB-303", "x/decodex-pub-303")
			.expect("stale historical lifecycle should read")
			.is_none(),
		"historical mixed review rows must not become lifecycle records"
	);

	drop(store);

	let connection = Connection::open(&state_path).expect("bootstrapped db should open");
	let legacy_table_count: i64 = connection
		.query_row(
			"SELECT COUNT(*) FROM sqlite_master \
			 WHERE type = 'table' AND name IN ('review_handoffs', 'review_orchestrations')",
			[],
			|row| row.get(0),
		)
		.expect("legacy marker tables should query");

	assert_eq!(legacy_table_count, 0);

	let lifecycle_count: i64 = connection
		.query_row("SELECT COUNT(*) FROM review_lifecycle_records", [], |row| row.get(0))
		.expect("review lifecycle rows should query");

	assert_eq!(lifecycle_count, 0);
}
