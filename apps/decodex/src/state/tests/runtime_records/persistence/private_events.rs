use tempfile::TempDir;

use crate::state::StateStore;

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
