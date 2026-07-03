use tempfile::TempDir;

use crate::{
	state::{ReviewPolicyCheckpointInput, StateStore, tests::runtime_records::IN_PROGRESS_STATE},
	tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};
#[test]
fn state_store_open_persists_runtime_history_across_instances() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let first = StateStore::open(&state_path).expect("first state store should open");

	first
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should persist");
	first.record_run_attempt("run-1", "PUB-101", 1, "running").expect("run attempt should record");
	first.update_run_thread("run-1", "thread-1").expect("thread should persist");
	first.append_event("run-1", 1, "thread/run/created", "{}").expect("event should persist");
	first
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should persist");

	let mut ledger_record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-101",
			issue_identifier: "PUB-101",
			run_id: "run-1",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-04-29T10:10:00Z"),
		"closeout",
	);

	ledger_record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/101"));
	ledger_record.commit_sha = Some(String::from("1111111111111111111111111111111111111111"));
	ledger_record.summary = Some(String::from("Completed retained closeout."));

	first
		.record_linear_execution_event(&ledger_record)
		.expect("linear execution event should persist");

	assert!(state_path.exists(), "persistent runtime DB should be created");

	let second = StateStore::open(&state_path).expect("second state store should open");
	let latest = second
		.latest_run_attempt_for_issue("PUB-101")
		.expect("latest run lookup should succeed")
		.expect("persistent store should recover run history");

	assert_eq!(latest.run_id(), "run-1");
	assert_eq!(latest.thread_id(), Some("thread-1"));
	assert_eq!(second.event_count("run-1").expect("event count should load"), 1);
	assert!(
		second.lease_for_issue("PUB-101").expect("lease lookup should succeed").is_some(),
		"persistent store should recover run leases"
	);
	assert!(
		second.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_some(),
		"persistent store should recover retained worktree mappings"
	);

	let ledger_records = second
		.list_linear_execution_events("pubfi", "PUB-101")
		.expect("linear execution events should load");

	assert_eq!(ledger_records, vec![ledger_record]);
}

#[test]
fn private_execution_events_persist_reload_and_keep_append_order() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let first = store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"evidence_snapshot",
			serde_json::json!({
				"summary": "first private snapshot",
				"evidence": ["runtime-db", "local-only"],
			}),
		)
		.expect("first private event should append");
	let second = store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"review_pass",
			serde_json::json!({
				"summary": "second private snapshot",
				"outcome": "clean",
			}),
		)
		.expect("second private event should append");

	assert!(
		first.record_id() < second.record_id(),
		"private event row ids should preserve append order"
	);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let events = reopened
		.list_private_execution_events("decodex", "XY-520", "run-1", 2)
		.expect("private events should reload");

	assert_eq!(events.len(), 2);
	assert_eq!(events[0].record_id(), first.record_id());
	assert_eq!(events[0].project_id(), "decodex");
	assert_eq!(events[0].issue_id(), "XY-520");
	assert_eq!(events[0].run_id(), "run-1");
	assert_eq!(events[0].attempt_number(), 2);
	assert_eq!(events[0].event_type(), "evidence_snapshot");
	assert_eq!(events[0].payload()["evidence"], serde_json::json!(["runtime-db", "local-only"]));
	assert_eq!(events[1].record_id(), second.record_id());
	assert_eq!(events[1].event_type(), "review_pass");
	assert_eq!(events[1].payload()["outcome"], serde_json::json!("clean"));
	assert!(events[0].recorded_at_unix() <= events[1].recorded_at_unix());
	assert!(!events[0].recorded_at().is_empty());
}

#[test]
fn project_loop_evidence_snapshot_filters_project_evidence_once() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let first = store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"evidence_snapshot",
			serde_json::json!({"match": true}),
		)
		.expect("first private event should append");
	let second = store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"terminal_finalize",
			serde_json::json!({"path": "review_handoff"}),
		)
		.expect("second private event should append");

	store
		.append_private_execution_event(
			"other",
			"XY-520",
			"run-1",
			2,
			"other_project",
			serde_json::json!({"match": false}),
		)
		.expect("other project private event should append");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "decodex",
			issue_id: "XY-520",
			run_id: "run-1",
			attempt_number: 2,
			phase: "handoff",
			review_level: "standard",
			status: "clean",
			head_sha: "abc123",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review policy checkpoint should persist");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "other",
			issue_id: "XY-520",
			run_id: "run-1",
			attempt_number: 2,
			phase: "handoff",
			review_level: "standard",
			status: "findings",
			head_sha: "def456",
			nonclean_rounds: 1,
			details_json: "{}",
		})
		.expect("other project checkpoint should persist");

	let snapshot = StateStore::open(&state_path)
		.expect("state store should reopen")
		.project_loop_evidence_snapshot("decodex")
		.expect("project loop evidence should load");
	let events = snapshot.private_events("XY-520", "run-1", 2);
	let checkpoint = snapshot
		.review_policy_checkpoint("XY-520", "run-1", 2, "handoff")
		.expect("matching checkpoint should exist");

	assert_eq!(
		events.iter().map(|event| event.record_id()).collect::<Vec<_>>(),
		vec![first.record_id(), second.record_id()],
		"snapshot should preserve append order and exclude other projects"
	);
	assert_eq!(events[1].event_type(), "terminal_finalize");
	assert_eq!(checkpoint.status(), "clean");
	assert!(snapshot.private_events("XY-521", "run-1", 2).is_empty());
}

#[test]
fn private_execution_events_filter_issue_run_attempt_and_stay_out_of_linear_cache() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			1,
			"kept",
			serde_json::json!({"match": true}),
		)
		.expect("matching private event should append");
	store
		.append_private_execution_event(
			"decodex",
			"XY-521",
			"run-1",
			1,
			"other_issue",
			serde_json::json!({"match": false}),
		)
		.expect("other issue private event should append");
	store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-2",
			1,
			"other_run",
			serde_json::json!({"match": false}),
		)
		.expect("other run private event should append");
	store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"other_attempt",
			serde_json::json!({"match": false}),
		)
		.expect("other attempt private event should append");
	store
		.append_private_execution_event(
			"pubfi",
			"XY-520",
			"run-1",
			1,
			"other_project",
			serde_json::json!({"match": false}),
		)
		.expect("other project private event should append");

	let events = store
		.list_private_execution_events("decodex", "XY-520", "run-1", 1)
		.expect("private events should list");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "kept");
	assert_eq!(events[0].payload()["match"], serde_json::json!(true));
	assert!(
		store
			.list_linear_execution_events("decodex", "XY-520")
			.expect("linear event cache should read")
			.is_empty(),
		"private execution events must not populate the public Linear mirror cache"
	);
}
