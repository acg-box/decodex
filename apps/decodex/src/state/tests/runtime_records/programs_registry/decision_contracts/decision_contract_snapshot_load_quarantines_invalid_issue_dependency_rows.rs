use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{StateStore, tests};

#[test]
fn decision_contract_snapshot_load_quarantines_invalid_issue_dependency_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			tests::latent_decision_contract_fixture(),
		)
		.expect("valid decision contract should persist");

	let mut invalid_payload = serde_json::to_value(tests::latent_decision_contract_fixture())
		.expect("fixture should encode as JSON");

	invalid_payload["contract_id"] = serde_json::json!("invalid-dependency-contract");
	invalid_payload["execution_readiness"]["proposed_issues"][0]["dependencies"] =
		serde_json::json!(["XY-952"]);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				"decodex",
				"invalid-dependency-contract",
				"XY-BROKEN",
				"draft_latent",
				serde_json::to_string(&invalid_payload)
					.expect("invalid dependency payload should serialize"),
				"2026-07-01T00:00:00Z",
				1_i64,
				"2026-07-01T00:00:00Z",
				1_i64,
			],
		)
		.expect("invalid dependency row should insert");

	let reopened =
		StateStore::open(&state_path).expect("invalid dependency contract should be quarantined");
	let valid_contract = reopened
		.decision_contract("decodex", "research-x-loop-contract")
		.expect("valid contract should remain readable")
		.expect("valid contract should exist");

	assert_eq!(valid_contract.contract_id(), "research-x-loop-contract");

	let project_contracts = reopened
		.list_decision_contracts_for_project("decodex")
		.expect("project contract list should skip invalid rows");

	assert_eq!(project_contracts.len(), 1);
	assert_eq!(project_contracts[0].contract_id(), "research-x-loop-contract");
	assert!(
		reopened
			.list_decision_contracts_for_issue("decodex", "XY-BROKEN")
			.expect("issue contract list should skip invalid rows")
			.is_empty()
	);
	assert!(
		reopened.decision_contract("decodex", "invalid-dependency-contract").is_err(),
		"direct reads of the invalid contract should still fail validation"
	);
}
