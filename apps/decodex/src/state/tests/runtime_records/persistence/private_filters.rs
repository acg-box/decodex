use crate::state::StateStore;

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
