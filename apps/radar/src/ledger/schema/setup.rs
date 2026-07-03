use std::{fs, path::Path};

use rusqlite::Connection;

use crate::{ledger::schema::migrations, prelude::Result};

pub(crate) fn open_ledger(path: &Path) -> Result<Connection> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let connection = Connection::open(path)?;

	initialize_ledger(&connection)?;

	Ok(connection)
}

pub(crate) fn initialize_ledger(connection: &Connection) -> Result<()> {
	connection.execute_batch(
		"
		PRAGMA foreign_keys = ON;

		CREATE TABLE IF NOT EXISTS metadata (
		  key TEXT PRIMARY KEY,
		  value TEXT NOT NULL
		);

		CREATE TABLE IF NOT EXISTS upstream_commit (
		  repo TEXT NOT NULL,
		  sha TEXT NOT NULL,
		  title TEXT NOT NULL,
		  url TEXT NOT NULL,
		  committed_at TEXT,
		  pr_number INTEGER,
		  first_seen_at TEXT NOT NULL,
		  last_seen_at TEXT NOT NULL,
		  PRIMARY KEY (repo, sha)
		);

		CREATE TABLE IF NOT EXISTS radar_review (
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

		CREATE TABLE IF NOT EXISTS artifact_link (
		  repo TEXT NOT NULL,
		  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
		  subject_id TEXT NOT NULL,
		  artifact_kind TEXT NOT NULL CHECK (
		    artifact_kind IN (
		      'bundle',
		      'analysis',
		      'signal',
		      'upstream_impact',
		      'control_plane_upgrade_candidate',
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

		CREATE TABLE IF NOT EXISTS source_cache (
		  url TEXT PRIMARY KEY,
		  etag TEXT,
		  body_sha256 TEXT NOT NULL,
		  fetched_at TEXT NOT NULL,
		  cache_path TEXT
		);

		CREATE INDEX IF NOT EXISTS idx_upstream_commit_pr
		  ON upstream_commit (repo, pr_number);

		CREATE INDEX IF NOT EXISTS idx_radar_review_status
		  ON radar_review (status, reviewed_at);
		",
	)?;

	migrations::migrate_radar_review_statuses(connection)?;
	migrations::migrate_artifact_link_kinds(connection)?;

	connection.execute(
		"
		INSERT INTO metadata (key, value)
		VALUES ('schema_version', ?1)
		ON CONFLICT(key) DO UPDATE SET value = excluded.value
		",
		rusqlite::params![crate::SCHEMA_VERSION.to_string()],
	)?;

	Ok(())
}
