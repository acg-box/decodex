use std::{fs, path::Path};

use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{
	self, StateStore,
	tests::{self, runtime_records::IN_PROGRESS_STATE},
};

#[test]
fn canonicalize_issue_identity_retargets_persistent_rows_without_cache_refresh() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let stale_store = StateStore::open(&state_path).expect("stale state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let handoff = tests::sample_pub_101_review_handoff();
	let orchestration = tests::sample_pub_101_review_orchestration();

	writer
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should persist");
	writer
		.upsert_worktree("pubfi", "PUB-101", "x/decodex-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should persist");
	writer
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should persist");
	writer
		.append_private_execution_event(
			"pubfi",
			"PUB-101",
			"run-1",
			1,
			"progress_checkpoint",
			serde_json::json!({ "summary": "cached on visible tracker key" }),
		)
		.expect("private evidence should persist");
	writer
		.upsert_decision_contract(
			"pubfi",
			Some("PUB-101"),
			tests::latent_decision_contract_fixture(),
		)
		.expect("decision contract should persist");
	writer
		.upsert_review_lifecycle_handoff_fixture("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	writer
		.upsert_review_lifecycle_transition_fixture("pubfi", "PUB-101", &orchestration)
		.expect("orchestration projection should persist");

	tests::upsert_handoff_review_policy_checkpoint(
		&writer,
		"PUB-101",
		"run-1",
		"findings",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		2,
	);

	stale_store
		.canonicalize_issue_identity("PUB-101", "linear-id-101")
		.expect("identity should canonicalize from SQLite rows");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let run = reopened
		.run_attempt("run-1")
		.expect("run attempt should read")
		.expect("run attempt should exist");

	assert_eq!(run.issue_id(), "linear-id-101");
	assert!(reopened.lease_for_issue("PUB-101").expect("old lease lookup should read").is_none());
	assert!(
		reopened.worktree_for_issue("PUB-101").expect("old worktree lookup should read").is_none()
	);
	assert_eq!(
		reopened
			.lease_for_issue("linear-id-101")
			.expect("canonical lease lookup should read")
			.expect("canonical lease should exist")
			.run_id(),
		"run-1"
	);
	assert_eq!(
		reopened
			.worktree_for_issue("linear-id-101")
			.expect("canonical worktree lookup should read")
			.expect("canonical worktree should exist")
			.branch_name(),
		"x/decodex-pub-101"
	);
	assert_eq!(
		reopened
			.list_private_execution_events("pubfi", "linear-id-101", "run-1", 1)
			.expect("canonical private evidence should read")
			.len(),
		3
	);

	tests::assert_decision_contract_retargeted(&reopened);

	assert_eq!(
		reopened
			.review_lifecycle_handoff_fixture("pubfi", "linear-id-101", "x/decodex-pub-101")
			.expect("canonical handoff should read"),
		Some(handoff.clone())
	);
	assert_eq!(
		reopened
			.review_lifecycle_transition_fixture("pubfi", "linear-id-101", &handoff)
			.expect("canonical orchestration should read"),
		Some(orchestration)
	);
	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 1, "handoff")
			.expect("old review policy checkpoint should read")
			.is_none()
	);

	let canonical_checkpoint = reopened
		.review_policy_checkpoint("pubfi", "linear-id-101", "run-1", 1, "handoff")
		.expect("canonical review policy checkpoint should read")
		.expect("canonical review policy checkpoint should exist");

	assert_eq!(canonical_checkpoint.status(), "findings");
	assert_eq!(canonical_checkpoint.nonclean_rounds(), 2);
}

#[test]
fn read_only_project_run_listing_does_not_persist_marker_identities() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let worktree_path = temp_dir.path().join("worktrees/PUB-101");
	let store = StateStore::open(&state_path).expect("state store should open");

	fs::create_dir_all(&worktree_path).expect("worktree should exist");

	store.record_run_attempt("run-1", "PUB-101", 1, "running").expect("run attempt should persist");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should persist");
	store
		.upsert_worktree(
			"pubfi",
			"PUB-101",
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should persist");

	state::write_run_thread_marker(&worktree_path, "run-1", 1, "thread-marker")
		.expect("thread marker should write");
	state::write_run_turn_marker(&worktree_path, "run-1", 1, "turn-marker")
		.expect("turn marker should write");

	let (leased_runs, _) =
		store.list_project_runs_read_only("pubfi", 0).expect("read-only runs should load");

	assert_eq!(leased_runs.len(), 1);
	assert_eq!(leased_runs[0].thread_id(), None);
	assert_eq!(leased_runs[0].turn_id(), None);

	assert_sqlite_run_attempt_identity(&state_path, None, None);

	store.list_project_runs("pubfi", 0).expect("ordinary runs should load");

	assert_sqlite_run_attempt_identity(&state_path, Some("thread-marker"), Some("turn-marker"));
}

fn assert_sqlite_run_attempt_identity(
	state_path: &Path,
	expected_thread_id: Option<&str>,
	expected_turn_id: Option<&str>,
) {
	let connection = Connection::open(state_path).expect("sqlite should open");
	let (thread_id, turn_id): (Option<String>, Option<String>) = connection
		.query_row(
			"SELECT thread_id, turn_id FROM run_attempts WHERE run_id = 'run-1'",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.expect("run attempt row should exist");

	assert_eq!(thread_id.as_deref(), expected_thread_id);
	assert_eq!(turn_id.as_deref(), expected_turn_id);
}
