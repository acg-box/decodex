use crate::{
	execution_program::{ExecutionProgram, ExecutionQueueIntent},
	orchestrator::tests::operator::status::{
		self, Connection, FakeTracker, StateStore, orchestrator, text::program_readback,
	},
};

#[test]
fn operator_status_json_surfaces_missing_contract_program_recovery() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let contract = program_readback::accepted_status_decision_contract_fixture();
	let program = ExecutionProgram::from_accepted_contract(
		"program-missing-contract",
		config.service_id(),
		&contract,
		vec![program_readback::status_program_node(
			"node-stale",
			"issue-stale",
			"PUB-946",
			"Todo",
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.program_id, "program-missing-contract");
	assert_eq!(program.status, "stale");
	assert_eq!(program.source_contract_id.as_deref(), Some(contract.contract_id()));
	assert_eq!(program.intake_kind.as_deref(), Some("goal_intake"));
	assert_eq!(program.stale_count, 1);
	assert_eq!(program.readback_warning.as_deref(), Some("source_decision_contract_missing"));
	assert_eq!(program.mapped_issue_identifiers, vec![String::from("PUB-946")]);

	let node = program.node_readbacks.first().expect("stale node should render");

	assert_eq!(node.lifecycle_state, "stale");
	assert_eq!(node.readiness_state, "stale");
	assert_eq!(node.issue_identifier.as_deref(), Some("PUB-946"));
	assert_eq!(node.reason_codes, vec![String::from("source_decision_contract_missing")]);
	assert!(node.next_action.contains("Decision Contract"));

	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let program_json = snapshot_json["execution_programs"]
		.as_array()
		.expect("execution programs should serialize as an array")
		.first()
		.expect("program should serialize");

	assert_eq!(program_json["status"], "stale");
	assert_eq!(program_json["readback_warning"], "source_decision_contract_missing");
	assert_eq!(program_json["node_readbacks"][0]["program_stage"], "runtime");
	assert_eq!(
		program_json["node_readbacks"][0]["reason_codes"][0],
		"source_decision_contract_missing"
	);
	assert_eq!(
		program_json["node_readbacks"][0]["next_action"],
		"Restore or supersede the source Decision Contract before dispatching this program."
	);
	assert!(program_json.get("contract").is_none());
	assert!(program_json.get("decision_contract").is_none());
}

#[test]
fn operator_status_readback_uses_migrated_removed_flat_decision_contract_fields() {
	let (temp_dir, config, workflow) = status::temp_project_layout();
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let state_store = StateStore::open(&state_path).expect("state store should open");
	let contract = program_readback::accepted_status_decision_contract_fixture();
	let program = ExecutionProgram::from_accepted_contract(
		"program-removed-flat-contract",
		config.service_id(),
		&contract,
		vec![program_readback::status_program_node(
			"node-removed-flat",
			"issue-removed-flat",
			"PUB-947",
			"Todo",
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let mut removed_field_payload =
		serde_json::to_value(&contract).expect("contract should encode as JSON");
	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Flat summary that must be migrated before readback."]),
	);
	readiness.insert(
		String::from("queue_intent"),
		serde_json::json!(["Removed queue intent that must not be re-admitted."]),
	);

	{
		let connection = Connection::open(&state_path).expect("sqlite should open");

		connection
			.execute(
				"INSERT INTO decision_contracts (
						project_id, contract_id, source_issue_id, status, payload_json, created_at,
						created_at_unix, updated_at, updated_at_unix
					) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
				rusqlite::params![
					config.service_id(),
					contract.contract_id(),
					"PUB-947",
					contract.status().as_str(),
					serde_json::to_string(&removed_field_payload)
						.expect("removed-field payload should serialize"),
					"2026-06-17T00:00:00Z",
					1_i64,
					"2026-06-17T00:00:00Z",
					1_i64,
				],
			)
			.expect("removed-field decision contract row should insert");
		connection
			.execute("UPDATE schema_meta SET value = '11' WHERE key = 'schema_version'", [])
			.expect("schema version should mark removed-field state");
	}

	drop(state_store);

	let state_store = StateStore::open(&state_path).expect("removed fields should migrate");
	let migrated_contract = state_store
		.decision_contract(config.service_id(), contract.contract_id())
		.expect("migrated contract read should succeed")
		.expect("migrated contract should exist");

	assert_eq!(
		migrated_contract.contract().execution_readiness().proposed_issues()[0].objective(),
		"Flat summary that must be migrated before readback."
	);

	let snapshot =
		program_readback::build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.program_id, "program-removed-flat-contract");
	assert_eq!(program.status, "stale");
	assert_ne!(program.readback_warning.as_deref(), Some("source_decision_contract_missing"));
	assert_eq!(program.stale_count, 1);
	assert_eq!(program.mapped_issue_identifiers, vec![String::from("PUB-947")]);
}
