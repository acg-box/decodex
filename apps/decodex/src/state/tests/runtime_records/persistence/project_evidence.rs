use tempfile::TempDir;

use crate::state::{ReviewPolicyCheckpointInput, StateStore};

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
