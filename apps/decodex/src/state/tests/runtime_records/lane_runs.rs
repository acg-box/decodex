use std::{fs, path::Path};

use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{
	self, ChildAgentActivityBucket, ChildAgentActivitySummary, StateStore,
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
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	writer
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
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
		1
	);

	tests::assert_decision_contract_retargeted(&reopened);

	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "linear-id-101", "x/decodex-pub-101")
			.expect("canonical handoff should read"),
		Some(handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "linear-id-101", &handoff)
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
fn lists_issue_leases() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("first lease should be inserted");
	store
		.upsert_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
		.expect("second lease should be inserted");

	let leases = store.list_leases("pubfi").expect("lease listing should succeed");

	assert_eq!(leases.len(), 2);
	assert_eq!(leases[0].project_id(), "pubfi");
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(leases[1].issue_id(), "PUB-102");
}

#[test]
fn lists_recent_project_runs_with_protocol_summary() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-2", "PUB-102", 2, "failed")
		.expect("older run attempt should be recorded");
	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("running run attempt should be recorded");
	store.update_run_thread("run-1", "thread-1").expect("thread id should attach");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("active worktree should record");
	store
		.upsert_worktree("pubfi", "PUB-102", "x/pubfi-pub-102", "/tmp/worktrees/pub-102")
		.expect("retained worktree should record");
	store
		.append_event("run-1", 1, "turn/started", "{\"turn\":\"1\"}")
		.expect("event should record");
	store
		.append_event("run-1", 2, "turn/completed", "{\"turn\":\"1\"}")
		.expect("second event should record");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 2);
	assert_eq!(runs[0].run_id(), "run-1");
	assert!(runs[0].run_lease());
	assert_eq!(runs[0].thread_id(), Some("thread-1"));
	assert_eq!(runs[0].event_count(), 2);
	assert_eq!(runs[0].last_event_type(), Some("turn/completed"));
	assert_eq!(runs[0].branch_name(), Some("x/pubfi-pub-101"));
	assert_eq!(runs[0].worktree_path(), Some(Path::new("/tmp/worktrees/pub-101")));
	assert_eq!(runs[1].run_id(), "run-2");
	assert!(!runs[1].run_lease());
	assert_eq!(runs[1].event_count(), 0);
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

#[test]
fn lists_project_issue_runs_recovered_from_local_evidence() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");
	let activity = ChildAgentActivitySummary {
		buckets: vec![ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 120,
			event_count: 2,
			tool_call_count: 1,
			input_tokens: 400,
			output_tokens: 80,
			..ChildAgentActivityBucket::default()
		}],
		wall_seconds: 120,
		event_count: 2,
		tool_call_count: 1,
		input_tokens_cumulative: 400,
		output_tokens_cumulative: 80,
		..ChildAgentActivitySummary::default()
	};

	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record");
	store
		.record_run_activity_summary("run-recovered", 1, Some(&activity), None)
		.expect("activity summary should record");
	store
		.append_event("run-recovered", 1, "turn/completed", "{}")
		.expect("protocol event should record");
	store
		.append_private_execution_event(
			"pubfi",
			"PUB-101",
			"run-recovered",
			1,
			"issue_progress_checkpoint",
			serde_json::json!({ "source": "test" }),
		)
		.expect("private execution evidence should record");

	let runs = store.list_project_issue_runs("pubfi", "PUB-101").expect("issue runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-recovered");
	assert_eq!(runs[0].attempt_number(), 1);
	assert_eq!(runs[0].status(), "recovered");
	assert_eq!(runs[0].recovery_source(), "recovered");
	assert!(
		runs[0]
			.recovery_evidence()
			.iter()
			.any(|evidence| evidence == "private_execution_event:issue_progress_checkpoint")
	);
	assert!(runs[0].recovery_evidence().iter().any(|evidence| evidence == "run_activity_summary"));
	assert!(runs[0].recovery_evidence().iter().any(|evidence| evidence == "protocol_events:1"));
	assert!(runs[0].recovery_gaps().is_empty());
	assert_eq!(runs[0].event_count(), 1);
	assert_eq!(runs[0].child_agent_activity().expect("activity should recover").event_count, 2);
}

#[test]
fn lists_recent_project_runs_after_terminal_lane_cleanup() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should record before project ownership is known");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record project ownership");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record project ownership");
	store.update_run_status("run-1", "succeeded").expect("terminal status should update");
	store.clear_lease("PUB-101").expect("terminal cleanup should clear run lease");
	store.clear_worktree("PUB-101").expect("terminal cleanup should clear worktree mapping");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert_eq!(runs[0].status(), "succeeded");
	assert!(!runs[0].run_lease());
	assert_eq!(runs[0].branch_name(), None);
	assert_eq!(runs[0].worktree_path(), None);
	assert!(
		store.list_recent_runs("other", 10).expect("other project lookup should load").is_empty(),
		"remembered run ownership must stay scoped to the original project"
	);
}

#[test]
fn lists_active_project_runs_only() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "running").expect("first run should record");
	store.record_run_attempt("run-2", "PUB-102", 1, "running").expect("second run should record");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.upsert_lease("other", "PUB-102", "run-2", IN_PROGRESS_STATE)
		.expect("other-project lease should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("first worktree should record");
	store
		.upsert_worktree("other", "PUB-102", "x/other-pub-102", "/tmp/worktrees/pub-102")
		.expect("second worktree should record");

	let runs = store.list_leased_runs("pubfi").expect("active project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert!(runs[0].run_lease());
}
