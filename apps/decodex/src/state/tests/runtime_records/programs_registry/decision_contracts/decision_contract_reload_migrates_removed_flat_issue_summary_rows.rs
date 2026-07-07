use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

use crate::state::{StateStore, tests};

#[test]
fn decision_contract_reload_migrates_removed_flat_issue_summary_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			tests::latent_decision_contract_fixture(),
		)
		.expect("current decision contract should persist");

	let mut removed_field_payload = serde_json::to_value(tests::latent_decision_contract_fixture())
		.expect("fixture should encode as JSON");

	removed_field_payload["contract_id"] = serde_json::json!("removed-flat-issue-contract");

	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Flat summary that must be migrated."]),
	);
	readiness.insert(
		String::from("queue_intent"),
		serde_json::json!(["Removed queue intent that must not be re-admitted."]),
	);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				"decodex",
				"removed-flat-issue-contract",
				"XY-OLD",
				"draft_latent",
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

	let reopened =
		StateStore::open(&state_path).expect("removed flat issue summary row should migrate");
	let project_contracts = reopened
		.list_decision_contracts_for_project("decodex")
		.expect("project contracts should list");
	let contract_ids =
		project_contracts.iter().map(|record| record.contract_id()).collect::<Vec<_>>();

	assert_eq!(project_contracts.len(), 2);
	assert!(contract_ids.contains(&"decision-x-loop-contract"));
	assert!(contract_ids.contains(&"removed-flat-issue-contract"));

	let migrated_contract = reopened
		.decision_contract("decodex", "removed-flat-issue-contract")
		.expect("migrated contract read should succeed")
		.expect("migrated contract should exist");

	assert_eq!(migrated_contract.source_issue_id(), Some("XY-OLD"));
	assert_eq!(migrated_contract.contract().execution_readiness().proposed_issues().len(), 1);
	assert_eq!(
		migrated_contract.contract().execution_readiness().proposed_issues()[0].objective(),
		"Flat summary that must be migrated."
	);

	let migrated_payload: String = connection
		.query_row(
			"SELECT payload_json FROM decision_contracts WHERE contract_id = 'removed-flat-issue-contract'",
			[],
			|row| row.get(0),
		)
		.expect("migrated payload should read");
	let migrated_value: Value =
		serde_json::from_str(&migrated_payload).expect("migrated payload should parse");

	assert!(
		migrated_value.pointer("/execution_readiness/proposed_issue_summaries").is_none(),
		"removed field should be absent after migration"
	);
	assert!(
		migrated_value.pointer("/execution_readiness/queue_intent").is_none(),
		"removed queue intent should be absent after migration"
	);
}
