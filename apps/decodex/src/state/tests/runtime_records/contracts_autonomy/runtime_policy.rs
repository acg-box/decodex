use rusqlite::Connection;
use tempfile::TempDir;

use crate::{
	autonomy_runtime_policy,
	state::{
		AutonomyRuntimePolicyReceiptInput, AutonomyRuntimePolicyRecord, StateStore,
		runtime_records::AutonomyRuntimePolicyRuntimeRecord,
	},
};

#[test]
fn autonomy_runtime_policy_exact_replay_is_idempotent_and_conflict_is_refused_in_memory() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let policy = runtime_policy_record("operator");
	let first =
		store.accept_autonomy_runtime_policy(policy.clone()).expect("runtime policy should accept");
	let replay = store
		.accept_autonomy_runtime_policy(policy.clone())
		.expect("exact runtime policy replay should be idempotent");

	assert_eq!(replay, first);
	assert_eq!(first.project_id(), "decodex");
	assert_eq!(first.policy_id(), "quality-autonomy-policy");
	assert_eq!(first.policy_version(), "1");
	assert_eq!(first.objective_id(), "quality-autonomy");
	assert_eq!(first.objective_version(), 1);
	assert_eq!(first.objective_digest(), "sha256:objective-fixture");
	assert_eq!(first.authority_ref(), "decodex.runtime_policy:quality-autonomy-policy@1");
	assert_eq!(first.accepted_by(), "operator");
	assert_eq!(first.accepted_at(), "2026-07-10T12:00:00Z");
	assert_eq!(first.acceptance_source(), "operator-acceptance");
	assert_eq!(first.public_non_goals(), ["No direct tracker mutation.", "No review bypass."]);

	let error = store
		.accept_autonomy_runtime_policy(runtime_policy_record("different-operator"))
		.expect_err("changed payload for the same policy key must be refused");

	assert!(error.to_string().contains("conflicts with its immutable accepted record"));
	assert_eq!(
		store
			.autonomy_runtime_policy("decodex", "quality-autonomy-policy", "1")
			.expect("runtime policy should read")
			.expect("runtime policy should exist"),
		policy
	);
}

#[test]
fn operator_receipt_is_principal_bound_and_single_use() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let policy = runtime_policy_record("operator");
	let digest = autonomy_runtime_policy::runtime_policy_candidate_digest(&policy)
		.expect("digest should compute");

	store
		.issue_autonomy_runtime_policy_receipt(AutonomyRuntimePolicyReceiptInput {
			project_id: "decodex",
			receipt_id: "receipt-1",
			principal: "operator",
			candidate_digest: &digest,
			candidate: &policy,
			created_at: "2026-07-10T12:00:00Z",
			expires_at_unix: autonomy_runtime_policy::operator_receipt_expiry_unix(),
		})
		.expect("receipt should persist");

	let mismatch = store
		.accept_autonomy_runtime_policy_with_receipt("decodex", "receipt-1", "other")
		.expect_err("wrong principal must not consume receipt");

	assert!(mismatch.to_string().contains("principal_mismatch"));
	assert_eq!(
		store
			.accept_autonomy_runtime_policy_with_receipt("decodex", "receipt-1", "operator")
			.expect("bound principal should consume receipt"),
		policy
	);

	let replay = store
		.accept_autonomy_runtime_policy_with_receipt("decodex", "receipt-1", "operator")
		.expect_err("receipt replay must fail");

	assert!(replay.to_string().contains("already_consumed"));
}

#[test]
fn operator_receipt_rejects_lifetimes_longer_than_ten_minutes() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let policy = runtime_policy_record("operator");
	let digest = autonomy_runtime_policy::runtime_policy_candidate_digest(&policy)
		.expect("digest should compute");
	let error = store
		.issue_autonomy_runtime_policy_receipt(AutonomyRuntimePolicyReceiptInput {
			project_id: "decodex",
			receipt_id: "receipt-too-long",
			principal: "operator",
			candidate_digest: &digest,
			candidate: &policy,
			created_at: "2026-07-10T12:00:00Z",
			expires_at_unix: autonomy_runtime_policy::operator_receipt_expiry_unix() + 1,
		})
		.expect_err("receipt longer than ten minutes must fail closed");

	assert!(error.to_string().contains("runtime_policy_receipt_expiry_invalid"));
}

#[test]
fn autonomy_runtime_policy_persists_across_restart_and_stale_store_replay() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first_store = StateStore::open(&state_path).expect("first state store should open");
	let stale_store = StateStore::open(&state_path).expect("stale state store should open");
	let policy = runtime_policy_record("operator");

	first_store
		.accept_autonomy_runtime_policy(policy.clone())
		.expect("runtime policy should persist");

	let replay = stale_store
		.accept_autonomy_runtime_policy(policy.clone())
		.expect("stale store exact replay should refresh and succeed");

	assert_eq!(replay, policy);

	let sqlite_error = stale_store
		.upsert_autonomy_runtime_policy_locked(&AutonomyRuntimePolicyRuntimeRecord::from(
			runtime_policy_record("different-operator"),
		))
		.expect_err("SQLite upsert must refuse a conflicting accepted policy");

	assert!(sqlite_error.to_string().contains("conflicts with its immutable accepted record"));

	let error = stale_store
		.accept_autonomy_runtime_policy(runtime_policy_record("different-operator"))
		.expect_err("stale store must refuse a conflicting accepted policy");

	assert!(error.to_string().contains("conflicts with its immutable accepted record"));

	drop(first_store);
	drop(stale_store);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let persisted = reopened
		.autonomy_runtime_policy("decodex", "quality-autonomy-policy", "1")
		.expect("persisted runtime policy should read")
		.expect("persisted runtime policy should exist");
	let row_count = Connection::open(&state_path)
		.expect("sqlite should open")
		.query_row(
			"SELECT COUNT(*) FROM autonomy_runtime_policies
			 WHERE project_id = ?1 AND policy_id = ?2 AND policy_version = ?3",
			rusqlite::params!["decodex", "quality-autonomy-policy", "1"],
			|row| row.get::<_, i64>(0),
		)
		.expect("runtime policy row count should query");

	assert_eq!(persisted, policy);
	assert_eq!(row_count, 1, "exact replay must retain one immutable row");
}

#[test]
fn runtime_policy_schema_upgrade_revokes_legacy_unbound_acceptance() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let connection = Connection::open(&state_path).expect("legacy state should open");

	connection
		.execute_batch(
			r#"
CREATE TABLE autonomy_runtime_policies (
	project_id TEXT NOT NULL,
	policy_id TEXT NOT NULL,
	policy_version TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	objective_version INTEGER NOT NULL,
	authority_ref TEXT NOT NULL,
	accepted_by TEXT NOT NULL,
	accepted_at TEXT NOT NULL,
	acceptance_source TEXT NOT NULL,
	public_non_goals_json TEXT NOT NULL,
	PRIMARY KEY (project_id, policy_id, policy_version)
);
INSERT INTO autonomy_runtime_policies (
	project_id, policy_id, policy_version, objective_id, objective_version, authority_ref,
	accepted_by, accepted_at, acceptance_source, public_non_goals_json
) VALUES (
	'decodex', 'legacy-policy', '1', 'legacy-objective', 1,
	'decodex.runtime_policy:legacy-policy@1', 'legacy-operator',
	'2026-07-10T12:00:00Z', 'legacy-acceptance', '["No review bypass."]'
);
"#,
		)
		.expect("legacy runtime policy schema should create");

	drop(connection);

	let store = StateStore::open(&state_path).expect("legacy state should upgrade and load");

	assert!(
		store
			.autonomy_runtime_policy("decodex", "legacy-policy", "1")
			.expect("upgraded runtime policy should read")
			.is_none(),
		"an acceptance without an Objective digest must be revoked"
	);

	let connection = Connection::open(&state_path).expect("upgraded state should open");
	let objective_digest_column_count = connection
		.query_row(
			"SELECT COUNT(*) FROM pragma_table_info('autonomy_runtime_policies') WHERE name = 'objective_digest'",
			[],
			|row| row.get::<_, i64>(0),
		)
		.expect("runtime policy schema should inspect");
	let policy_count = connection
		.query_row("SELECT COUNT(*) FROM autonomy_runtime_policies", [], |row| row.get::<_, i64>(0))
		.expect("runtime policy rows should count");

	assert_eq!(objective_digest_column_count, 1);
	assert_eq!(policy_count, 0);
}

fn runtime_policy_record(accepted_by: &str) -> AutonomyRuntimePolicyRecord {
	AutonomyRuntimePolicyRecord::new(
		"decodex",
		"quality-autonomy-policy",
		"1",
		"quality-autonomy",
		1,
		"sha256:objective-fixture",
		"decodex.runtime_policy:quality-autonomy-policy@1",
		accepted_by,
		"2026-07-10T12:00:00Z",
		"operator-acceptance",
		vec![String::from("No direct tracker mutation."), String::from("No review bypass.")],
	)
	.expect("runtime policy fixture should validate")
}
