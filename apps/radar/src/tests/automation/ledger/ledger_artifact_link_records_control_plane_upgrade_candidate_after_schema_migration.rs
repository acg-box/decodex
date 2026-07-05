use std::fs;

use rusqlite::Connection;

use crate::{RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest};

#[test]
fn ledger_artifact_link_records_control_plane_upgrade_candidate_after_schema_migration() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let db_path = temp_dir.path().join("radar.sqlite3");
	let candidate_path = temp_dir.path().join("upgrade.json");
	let connection = Connection::open(&db_path).expect("temporary ledger should open");

	connection
		.execute_batch(
			"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				",
		)
		.expect("legacy artifact link schema should be created");

	drop(connection);

	fs::write(&candidate_path, r#"{"schema":"control_plane_upgrade_candidate/v1"}"#)
		.expect("upgrade candidate fixture should be written");
	crate::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
		.expect("ledger bootstrap should add control-plane upgrade artifact kind");

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
}
