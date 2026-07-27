use std::os::unix::fs::PermissionsExt as _;

use rusqlite::Connection;

use crate::RadarLedgerBootstrapRequest;

#[test]
fn ledger_bootstrap_rejects_obsolete_schema() {
	let temp_dir = crate::test_support::private_tempdir();
	std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
		.expect("ledger directory should be private");
	let db_path = temp_dir.path().join("radar.sqlite3");
	let connection = Connection::open(&db_path).expect("temporary ledger should open");

	connection
		.execute_batch(
			"
			CREATE TABLE metadata (
			  key TEXT PRIMARY KEY,
			  value TEXT NOT NULL
			);
			INSERT INTO metadata (key, value) VALUES ('schema_version', '5');
			",
		)
		.expect("obsolete schema should be created");
	drop(connection);
	std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600))
		.expect("obsolete ledger should be private");

	let error = crate::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path })
		.expect_err("obsolete schema must not be migrated");

	assert!(
		error.to_string().contains("unsupported Radar ledger schema"),
		"unexpected error: {error}"
	);
}

#[test]
fn ledger_bootstrap_rejects_forged_current_version_with_obsolete_constraints() {
	let temp_dir = crate::test_support::private_tempdir();
	std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
		.expect("ledger directory should be private");
	let db_path = temp_dir.path().join("radar.sqlite3");
	let connection = Connection::open(&db_path).expect("temporary ledger should open");

	connection
		.execute_batch(
			"
			CREATE TABLE metadata (
			  key TEXT PRIMARY KEY,
			  value TEXT NOT NULL
			);
			INSERT INTO metadata (key, value) VALUES ('schema_version', '6');
			CREATE TABLE artifact_link (
			  repo TEXT NOT NULL,
			  subject_kind TEXT NOT NULL,
			  subject_id TEXT NOT NULL,
			  artifact_kind TEXT NOT NULL CHECK (
			    artifact_kind IN ('bundle', 'archive_manifest')
			  ),
			  path TEXT NOT NULL,
			  sha256 TEXT NOT NULL,
			  size_bytes INTEGER NOT NULL,
			  created_at TEXT NOT NULL
			);
			",
		)
		.expect("forged schema should be created");
	drop(connection);
	std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600))
		.expect("forged ledger should be private");

	let error = crate::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path })
		.expect_err("forged current-version schema must fail");

	assert!(error.to_string().contains("structure is not canonical"), "unexpected error: {error}");
}

#[test]
fn ledger_bootstrap_preserves_quoted_literal_case_in_schema_attestation() {
	let temp_dir = crate::test_support::private_tempdir();

	std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
		.expect("ledger directory should be private");
	let db_path = temp_dir.path().join("radar.sqlite3");
	let initialized = crate::ledger::open_ledger(&db_path).expect("canonical ledger should open");

	initialized.close().expect("canonical ledger should persist");
	let connection = Connection::open(&db_path).expect("raw ledger should open");

	connection
		.execute_batch(
			"
			PRAGMA writable_schema = ON;
			UPDATE sqlite_master
			SET sql = replace(sql, '''seen''', '''SEEN''')
			WHERE type = 'table' AND name = 'radar_review';
			PRAGMA writable_schema = OFF;
			",
		)
		.expect("quoted status literal should be forged");
	let forged_sql: String = connection
		.query_row(
			"SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'radar_review'",
			[],
			|row| row.get(0),
		)
		.expect("forged schema SQL should be readable");

	assert!(forged_sql.contains("'SEEN'"));
	drop(connection);
	let error = crate::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path })
		.expect_err("case-distinct quoted constraints must fail exact attestation");

	assert!(error.to_string().contains("structure is not canonical"), "unexpected error: {error}");
}
