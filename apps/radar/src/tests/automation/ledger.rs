use std::fs;

use rusqlite::Connection;

use crate::{
	RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest, RadarLedgerIngestExistingRequest,
	tests::fixtures,
};

#[test]
fn ledger_bootstrap_drops_legacy_publisher_artifact_links_and_status() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let db_path = temp_dir.path().join("radar.sqlite3");
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
				      'publisher_handoff',
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
				CREATE TABLE radar_review (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  status TEXT NOT NULL CHECK (
				    status IN (
				      'seen',
				      'skipped',
				      'watch',
				      'signal',
				      'control_plane',
				      'publisher_handoff',
				      'deprecated',
				      'archived'
				    )
				  ),
				  reason TEXT NOT NULL DEFAULT '',
				  confidence TEXT CHECK (confidence IN ('confirmed', 'likely', 'weak')),
				  reviewed_at TEXT NOT NULL,
				  updated_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id)
				);
				INSERT INTO artifact_link (
				  repo,
				  subject_kind,
				  subject_id,
				  artifact_kind,
				  path,
				  sha256,
				  size_bytes,
				  created_at
				)
				VALUES (
				  'openai/codex',
				  'pr',
				  '22414',
				  'publisher_handoff',
				  '.agent/automations/decodex/cache/publisher/handoffs/example.json',
				  'abc123',
				  10,
				  '2026-06-01T00:00:00Z'
				);
				INSERT INTO radar_review (
				  repo,
				  subject_kind,
				  subject_id,
				  status,
				  reason,
				  confidence,
				  reviewed_at,
				  updated_at
				)
				VALUES (
				  'openai/codex',
				  'pr',
				  '22414',
				  'publisher_handoff',
				  'legacy publisher handoff',
				  'likely',
				  '2026-06-01T00:00:00Z',
				  '2026-06-01T00:00:00Z'
				);
				",
		)
		.expect("legacy artifact link schema should be created");

	drop(connection);

	crate::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
		.expect("ledger bootstrap should migrate legacy publisher rows");

	let connection = Connection::open(&db_path).expect("migrated ledger should open");
	let artifact_links: i64 = connection
		.query_row("SELECT COUNT(*) FROM artifact_link", [], |row| row.get(0))
		.expect("artifact link count should be readable");
	let review_status: String = connection
		.query_row("SELECT COALESCE(MAX(status), '') FROM radar_review", [], |row| row.get(0))
		.expect("review status count should be readable");
	let schema_version: String = connection
		.query_row("SELECT value FROM metadata WHERE key = 'schema_version'", [], |row| row.get(0))
		.expect("schema version should be readable");

	assert_eq!(artifact_links, 0);
	assert_eq!(review_status, "");
	assert_eq!(schema_version, "5");
}

#[test]
fn ledger_ingests_existing_bundle_analysis_and_signal_artifacts() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundles_dir = temp_dir.path().join("bundles");
	let analysis_dir = temp_dir.path().join("analysis");
	let signals_dir = temp_dir.path().join("signals");
	let db_path = temp_dir.path().join("radar.sqlite3");

	fs::create_dir_all(&bundles_dir).expect("bundles directory should be created");
	fs::create_dir_all(&analysis_dir).expect("analysis directory should be created");
	fs::create_dir_all(&signals_dir).expect("signals directory should be created");
	fs::write(bundles_dir.join("openai-codex-pr-22414.json"), fixtures::valid_bundle().to_string())
		.expect("bundle fixture should be written");
	fs::write(analysis_dir.join("openai-codex-pr-22414.analysis.json"), r#"{"kind":"capability"}"#)
		.expect("analysis fixture should be written");
	fs::write(signals_dir.join("openai-codex-pr-22414.json"), fixtures::valid_signal().to_string())
		.expect("signal fixture should be written");

	let summary = crate::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
		db_path: db_path.clone(),
		bundles_dir,
		analysis_dir,
		signals_dir,
	})
	.expect("existing artifacts should ingest");

	assert_eq!(summary.get("bundles_ingested"), Some(&1));
	assert_eq!(summary.get("upstream_commits"), Some(&1));
	assert_eq!(summary.get("radar_reviews"), Some(&1));
	assert_eq!(summary.get("artifact_links"), Some(&3));

	let connection = Connection::open(&db_path).expect("ingested ledger should open");
	let review: (String, String) = connection
		.query_row(
			"SELECT status, confidence FROM radar_review WHERE subject_kind = 'pr'",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.expect("review row should be readable");

	assert_eq!(review, ("signal".into(), "confirmed".into()));
}

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
