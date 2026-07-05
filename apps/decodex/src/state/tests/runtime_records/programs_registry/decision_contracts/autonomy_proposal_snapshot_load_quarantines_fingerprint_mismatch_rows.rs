use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{StateStore, tests};

#[test]
fn autonomy_proposal_snapshot_load_quarantines_fingerprint_mismatch_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let _store = StateStore::open(&state_path).expect("state store should open");
	let proposal = tests::autonomy_proposal_fixture();
	let mut invalid_payload =
		serde_json::to_value(&proposal).expect("proposal should encode as JSON");

	invalid_payload["affected_identifiers"] = serde_json::json!(["OperatorLoopStatus"]);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
			rusqlite::params![
				"decodex",
				proposal.id(),
				proposal.objective_id(),
				1_i64,
				proposal.state().as_str(),
				proposal.fingerprint(),
				proposal.source_family(),
				proposal.intended_surface(),
				serde_json::to_string(&invalid_payload)
					.expect("invalid proposal payload should serialize"),
				"2026-07-01T00:00:00Z",
				1_i64,
				"2026-07-01T00:00:00Z",
				1_i64,
			],
		)
		.expect("invalid proposal row should insert");

	let reopened =
		StateStore::open(&state_path).expect("invalid proposal should be quarantined on open");

	assert!(
		reopened
			.recent_autonomy_proposals_for_project("decodex", 10)
			.expect("recent proposal list should skip invalid rows")
			.is_empty()
	);
	assert!(
		reopened.autonomy_proposal("decodex", proposal.id()).is_err(),
		"direct reads of the invalid proposal should still fail validation"
	);
}
