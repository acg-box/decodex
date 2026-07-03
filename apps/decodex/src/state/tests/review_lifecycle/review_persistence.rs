use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{
	ReviewCheckpointArtifactLookup, ReviewHandoffMarker, ReviewOrchestrationMarker,
	ReviewPolicyCheckpointInput, StateStore, tests,
};

#[test]
fn clear_review_lifecycle_for_handoff_preserves_other_branches() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let removed_handoff = tests::sample_pub_101_review_handoff();
	let removed_orchestration = tests::sample_pub_101_review_orchestration();
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

	tests::upsert_handoff_review_policy_checkpoint(
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

	tests::upsert_handoff_review_policy_checkpoint(
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
