use rusqlite::Connection;

use crate::RadarLedgerBootstrapRequest;

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
