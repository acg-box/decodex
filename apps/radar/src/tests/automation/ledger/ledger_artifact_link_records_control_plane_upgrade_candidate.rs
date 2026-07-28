use std::{fs, os::unix::fs::PermissionsExt as _};

use rusqlite::Connection;

use crate::{RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest};

#[test]
fn ledger_artifact_link_records_control_plane_upgrade_candidate() {
	let temp_dir = crate::test_support::private_tempdir();
	fs::set_permissions(temp_dir.path(), fs::Permissions::from_mode(0o700))
		.expect("ledger directory should be private");
	let db_path = temp_dir.path().join("radar.sqlite3");
	let candidate_path = temp_dir.path().join("upgrade.json");

	fs::write(&candidate_path, r#"{"schema":"control_plane_upgrade_candidate/v1"}"#)
		.expect("upgrade candidate fixture should be written");
	crate::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
		.expect("ledger bootstrap should create the current schema");

	let summary = crate::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
		db_path: db_path.clone(),
		repo: "openai/codex".into(),
		subject_kind: "pr".into(),
		subject_id: "22414".into(),
		artifact_kind: "control_plane_upgrade_candidate".into(),
		path: candidate_path,
	})
	.expect("control-plane upgrade artifact link should be recorded");

	assert_eq!(summary.get("artifact_links"), Some(&1));

	let connection = Connection::open(&db_path).expect("ledger should reopen");
	let schema_version: String = connection
		.query_row("SELECT value FROM metadata WHERE key = 'schema_version'", [], |row| row.get(0))
		.expect("schema version should be readable");

	assert_eq!(schema_version, crate::SCHEMA_VERSION.to_string());
}
