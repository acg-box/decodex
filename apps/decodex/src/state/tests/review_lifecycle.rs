#[test]
fn loop_guardrail_checkpoints_track_fingerprints_and_retarget_issue() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let first = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-a",
			run_id: "run-1",
			attempt_number: 1,
			details_json: "{}",
		})
		.expect("first loop guardrail observation should persist");

	assert_eq!(first.consecutive_count(), 1);
	assert_eq!(first.reason(), "validation_repeat");

	let second = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-a",
			run_id: "run-2",
			attempt_number: 2,
			details_json: "{\"attempt\":2}",
		})
		.expect("same fingerprint should increment");

	assert_eq!(second.consecutive_count(), 2);
	assert_eq!(second.run_id(), "run-2");
	assert_eq!(second.attempt_number(), 2);
	assert!(second.updated_at_unix() > 0);

	let reset = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-b",
			run_id: "run-3",
			attempt_number: 3,
			details_json: "{\"attempt\":3}",
		})
		.expect("new fingerprint should reset");

	assert_eq!(reset.consecutive_count(), 1);
	assert_eq!(reset.fingerprint(), "fp-b");
	assert_eq!(reset.details_json(), "{\"attempt\":3}");
	assert!(!reset.updated_at().is_empty());

	store
		.canonicalize_issue_identity("PUB-101", "linear-id-101")
		.expect("issue identity should retarget");

	assert!(
		store
			.loop_guardrail_checkpoint("pubfi", "PUB-101", "validation_repeat")
			.expect("old checkpoint should read")
			.is_none(),
		"legacy issue identity should be cleared after retarget"
	);

	let canonical = store
		.loop_guardrail_checkpoint("pubfi", "linear-id-101", "validation_repeat")
		.expect("canonical checkpoint should read")
		.expect("canonical checkpoint should exist");

	assert_eq!(canonical.project_id(), "pubfi");
	assert_eq!(canonical.issue_id(), "linear-id-101");
	assert_eq!(canonical.fingerprint(), "fp-b");
	assert_eq!(canonical.consecutive_count(), 1);

	store
		.clear_loop_guardrail_checkpoints_for_issue("pubfi", "linear-id-101")
		.expect("checkpoint should clear");

	assert!(
		store
			.loop_guardrail_checkpoint("pubfi", "linear-id-101", "validation_repeat")
			.expect("cleared checkpoint should read")
			.is_none()
	);
}

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

	seed_dropped_review_marker_tables(&state_path);

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

#[test]
fn connector_backoff_roundtrip_and_clear_from_runtime_store() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_connector_backoff(ConnectorBackoffInput {
			project_id: "pubfi",
			connector: "linear",
			sync_phase: "post_review_lane_status",
			quota_class: "linear_graphql_rate_limit",
			reset_unix_epoch: 1_777_392_000,
			reset_source: "linear",
			warning: "tracker_rate_limited",
		})
		.expect("connector backoff should persist");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let backoff = reopened
		.connector_backoff("pubfi", "linear")
		.expect("connector backoff should read")
		.expect("connector backoff should exist");

	assert_eq!(backoff.project_id(), "pubfi");
	assert_eq!(backoff.connector(), "linear");
	assert_eq!(backoff.sync_phase(), "post_review_lane_status");
	assert_eq!(backoff.quota_class(), "linear_graphql_rate_limit");
	assert_eq!(backoff.reset_unix_epoch(), 1_777_392_000);
	assert_eq!(backoff.reset_source(), "linear");
	assert_eq!(backoff.warning(), "tracker_rate_limited");

	reopened.clear_connector_backoff("pubfi", "linear").expect("connector backoff should clear");

	let reopened = StateStore::open(&state_path).expect("state store should reopen again");

	assert!(
		reopened
			.connector_backoff("pubfi", "linear")
			.expect("connector backoff should read after clear")
			.is_none()
	);
}

#[test]
fn clear_review_lifecycle_for_handoff_preserves_other_branches() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let removed_handoff = sample_pub_101_review_handoff();
	let removed_orchestration = sample_pub_101_review_orchestration();
	let kept_handoff = ReviewHandoffMarker::new(
		"run-2",
		1,
		"x/decodex-pub-101-review",
		"https://github.com/hack-ink/decodex/pull/102",
		"main",
		"x/decodex-pub-101-review",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let kept_orchestration = ReviewOrchestrationMarker::new(
		"run-2",
		1,
		"x/decodex-pub-101-review",
		"https://github.com/hack-ink/decodex/pull/102",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &removed_handoff)
		.expect("removed handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &removed_orchestration)
		.expect("removed orchestration projection should persist");

	upsert_handoff_review_policy_checkpoint(
		&store,
		"PUB-101",
		"run-1",
		"findings",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		2,
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &kept_handoff)
		.expect("kept handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &kept_orchestration)
		.expect("kept orchestration projection should persist");

	upsert_handoff_review_policy_checkpoint(
		&store,
		"PUB-101",
		"run-2",
		"clean",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		0,
	);

	store
		.clear_review_lifecycle_for_handoff(
			"pubfi",
			"PUB-101",
			&removed_handoff,
			&removed_orchestration,
		)
		.expect("exact review lifecycle should clear");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("removed handoff projection should read")
			.is_none()
	);
	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101-review")
			.expect("kept handoff projection should read"),
		Some(kept_handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &kept_handoff)
			.expect("kept orchestration projection should read"),
		Some(kept_orchestration)
	);
	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 1, "handoff")
			.expect("removed review policy checkpoint should read")
			.is_none()
	);

	let kept_checkpoint = reopened
		.review_policy_checkpoint("pubfi", "PUB-101", "run-2", 1, "handoff")
		.expect("kept review policy checkpoint should read")
		.expect("kept review policy checkpoint should exist");

	assert_eq!(kept_checkpoint.status(), "clean");
	assert_eq!(kept_checkpoint.head_sha(), "18a20f7dfb9526e7421a5f095b1c6adec84e52d6");
}

#[test]
fn missing_review_lifecycle_projections_return_absent() {
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

	assert!(
		store
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("review handoff projection should read")
			.is_none()
	);
	assert!(
		store
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("review orchestration projection should read")
			.is_none()
	);
}

#[test]
fn review_policy_checkpoints_persist_reload_and_clear_for_run_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let checkpoint = store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			run_id: "run-1",
			attempt_number: 2,
			phase: "handoff",
			review_level: "standard",
			status: "findings",
			head_sha: "abc123",
			nonclean_rounds: 2,
			details_json: r#"{"reviewer":"independent_fresh_context"}"#,
		})
		.expect("review policy checkpoint should persist");

	assert_eq!(checkpoint.project_id(), "pubfi");
	assert_eq!(checkpoint.issue_id(), "PUB-101");
	assert_eq!(checkpoint.run_id(), "run-1");
	assert_eq!(checkpoint.attempt_number(), 2);
	assert_eq!(checkpoint.phase(), "handoff");
	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.head_sha(), "abc123");
	assert_eq!(checkpoint.nonclean_rounds(), 2);
	assert_eq!(checkpoint.details_json(), r#"{"reviewer":"independent_fresh_context"}"#);
	assert!(!checkpoint.updated_at().is_empty());
	assert!(checkpoint.updated_at_unix() > 0);

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");
	let reloaded = reopened
		.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 2, "handoff")
		.expect("review policy checkpoint should read")
		.expect("review policy checkpoint should exist");

	assert_eq!(reloaded.status(), "findings");
	assert_eq!(reloaded.nonclean_rounds(), 2);
	assert_eq!(reloaded.details_json(), r#"{"reviewer":"independent_fresh_context"}"#);

	reopened
		.clear_review_policy_checkpoints_for_run_attempt("pubfi", "PUB-101", "run-1", 2)
		.expect("review policy checkpoint should clear");

	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 2, "handoff")
			.expect("cleared review policy checkpoint should read")
			.is_none()
	);
}

#[test]
fn review_checkpoint_artifact_reuses_only_matching_evidence_key() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			run_id: "run-1",
			attempt_number: 2,
			phase: "handoff",
			review_level: "standard",
			status: "clean",
			head_sha: "abc123",
			nonclean_rounds: 0,
			details_json: r#"{"reviewer":"independent_fresh_context"}"#,
		})
		.expect("review policy checkpoint should persist");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let reused = reopened
		.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
			project_id: "pubfi",
			issue_id: "PUB-101",
			phase: "handoff",
			review_level: "standard",
			head_sha: "abc123",
		})
		.expect("review checkpoint artifact should read")
		.expect("matching artifact should exist");

	assert_eq!(reused.run_id(), "run-1");
	assert_eq!(reused.attempt_number(), 2);
	assert_eq!(reused.status(), "clean");
	assert_eq!(reused.details_json(), r#"{"reviewer":"independent_fresh_context"}"#);

	let key_json = Connection::open(&state_path)
		.expect("state sqlite should open")
		.query_row("SELECT key_json FROM evidence_artifacts", [], |row| row.get::<_, String>(0))
		.expect("review artifact key should read");

	assert!(key_json.contains(r#""review_prompt_version":"decodex-review-checkpoint/2""#));
	assert!(
		reopened
			.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
				project_id: "pubfi",
				issue_id: "PUB-101",
				phase: "handoff",
				review_level: "standard",
				head_sha: "def456",
			})
			.expect("wrong head lookup should read")
			.is_none()
	);
	assert!(
		reopened
			.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
				project_id: "pubfi",
				issue_id: "PUB-101",
				phase: "handoff",
				review_level: "strict",
				head_sha: "abc123",
			})
			.expect("wrong review-level lookup should read")
			.is_none()
	);
}

#[test]
fn corrupted_review_checkpoint_artifact_payload_fails_closed() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			run_id: "run-1",
			attempt_number: 2,
			phase: "handoff",
			review_level: "standard",
			status: "clean",
			head_sha: "abc123",
			nonclean_rounds: 0,
			details_json: r#"{"reviewer":"independent_fresh_context"}"#,
		})
		.expect("review policy checkpoint should persist");

	Connection::open(&state_path)
		.expect("state sqlite should open")
		.execute("UPDATE evidence_artifacts SET payload_json = 'not-json'", [])
		.expect("artifact payload should corrupt");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let error = reopened
		.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
			project_id: "pubfi",
			issue_id: "PUB-101",
			phase: "handoff",
			review_level: "standard",
			head_sha: "abc123",
		})
		.expect_err("corrupted artifact payload should fail closed");

	assert!(error.to_string().contains("Invalid review checkpoint artifact payload"));
}

#[test]
fn persistent_review_lifecycle_survives_stale_store_persist_and_is_visible() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	writer
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	writer
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration projection should persist");

	let observed_handoff = observer
		.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("observer should read handoff projection")
		.expect("observer should see lifecycle written by another store");

	assert_eq!(observed_handoff, handoff);

	observer
		.record_run_attempt("run-2", "PUB-202", 1, "running")
		.expect("stale observer should persist unrelated runtime state");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("reopened store should read handoff projection"),
		Some(handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("reopened store should read orchestration projection"),
		Some(orchestration)
	);
	assert!(
		reopened.run_attempt("run-2").expect("run attempt should read").is_some(),
		"unrelated stale-store persist should still keep its own update"
	);
}
