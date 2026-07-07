use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

use crate::state::{StateStore, tests};

#[test]
fn execution_programs_persist_reload_and_list_by_contract() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let mut contract = tests::latent_decision_contract_fixture();

	contract.promote(tests::sample_decision_promotion()).expect("contract should promote");

	let program = tests::sample_execution_program(&contract);
	let record = store
		.upsert_execution_program("decodex", program)
		.expect("execution program should persist");

	assert_eq!(record.project_id(), "decodex");
	assert_eq!(record.program_id(), "program-853");
	assert_eq!(record.source_contract_id(), Some("decision-x-loop-contract"));
	assert_eq!(record.program().nodes().len(), 1);
	assert!(record.created_at_unix() > 0);
	assert!(record.updated_at_unix() >= record.created_at_unix());

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let reloaded = reopened
		.execution_program("decodex", "program-853")
		.expect("execution program should read")
		.expect("execution program should exist");

	assert_eq!(reloaded.created_at(), record.created_at());
	assert_eq!(reloaded.program().source_contract_id(), Some("decision-x-loop-contract"));

	let contract_programs = reopened
		.list_execution_programs_for_contract("decodex", "decision-x-loop-contract")
		.expect("contract programs should list");

	assert_eq!(contract_programs.len(), 1);
	assert_eq!(contract_programs[0].program_id(), "program-853");

	let project_programs =
		reopened.list_execution_programs("decodex").expect("project programs should list");

	assert_eq!(project_programs.len(), 1);
	assert_eq!(project_programs[0].program_id(), "program-853");

	let intake_plans =
		reopened.list_program_intake_plans("decodex").expect("program intake plans should list");

	assert_eq!(intake_plans.len(), 1);
	assert_eq!(intake_plans[0].program_id(), "program-853");
	assert_eq!(intake_plans[0].intake_kind(), "goal_intake");
	assert_eq!(intake_plans[0].source_contract_id(), Some("decision-x-loop-contract"));

	let issue_mappings = reopened
		.list_program_issue_mappings("decodex", "program-853")
		.expect("program issue mappings should list");

	assert_eq!(issue_mappings.len(), 1);
	assert_eq!(issue_mappings[0].node_id(), "runtime-readiness");
	assert_eq!(issue_mappings[0].issue_identifier(), "XY-853");
	assert_eq!(issue_mappings[0].queue_intent(), "ready_to_queue");
	assert!(!issue_mappings[0].has_active_label());
}

#[test]
fn execution_program_reload_rejects_row_key_payload_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let mut contract = tests::latent_decision_contract_fixture();

	contract.promote(tests::sample_decision_promotion()).expect("contract should promote");
	store
		.upsert_execution_program("decodex", tests::sample_execution_program(&contract))
		.expect("execution program should persist");

	let connection = Connection::open(&state_path).expect("sqlite should open");
	let mut payload: Value = serde_json::from_str(
		&connection
			.query_row(
				"SELECT payload_json FROM execution_programs WHERE program_id = ?1",
				["program-853"],
				|row| row.get::<_, String>(0),
			)
			.expect("payload should load"),
	)
	.expect("payload should parse");

	payload["program_id"] = serde_json::json!("program-mismatch");

	connection
		.execute(
			"UPDATE execution_programs SET payload_json = ?1 WHERE program_id = ?2",
			[
				serde_json::to_string(&payload).expect("payload should serialize"),
				String::from("program-853"),
			],
		)
		.expect("payload should corrupt");

	assert!(
		StateStore::open(&state_path).is_err(),
		"execution program row key must match the versioned payload program_id"
	);
}

#[test]
fn decision_contract_reload_rejects_row_key_payload_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			tests::latent_decision_contract_fixture(),
		)
		.expect("latent decision contract should persist");

	let mut payload = serde_json::from_str::<Value>(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/decision_x_latent_contract.json"
	)))
	.expect("fixture should parse as JSON");

	payload["contract_id"] = serde_json::json!("mismatched-contract-id");

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"UPDATE decision_contracts SET payload_json = ?1 WHERE contract_id = ?2",
			rusqlite::params![
				serde_json::to_string(&payload).expect("payload should serialize"),
				"decision-x-loop-contract",
			],
		)
		.expect("decision contract row should corrupt for test");

	assert!(
		StateStore::open(&state_path).is_err(),
		"decision contract row key must match the versioned payload contract_id"
	);
}
