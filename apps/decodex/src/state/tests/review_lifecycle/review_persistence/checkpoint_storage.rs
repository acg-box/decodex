use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{ReviewCheckpointArtifactLookup, ReviewPolicyCheckpointInput, StateStore};

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
