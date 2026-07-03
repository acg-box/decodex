use rusqlite::{Connection, OptionalExtension as _};

use crate::prelude::Result;

pub(super) fn migrate_artifact_link_kinds(connection: &Connection) -> Result<()> {
	let table_sql = connection
		.query_row(
			"
			SELECT sql
			FROM sqlite_master
			WHERE type = 'table' AND name = 'artifact_link'
			",
			[],
			|row| row.get::<_, String>(0),
		)
		.optional()?;

	if table_sql.is_none() {
		return Ok(());
	};

	connection.execute_batch(
		"
		ALTER TABLE artifact_link RENAME TO artifact_link_old;

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

		INSERT OR REPLACE INTO artifact_link (
		  repo,
		  subject_kind,
		  subject_id,
		  artifact_kind,
		  path,
		  sha256,
		  size_bytes,
		  created_at
		)
		SELECT
		  repo,
		  subject_kind,
		  subject_id,
		  artifact_kind,
		  path,
		  sha256,
		  size_bytes,
		  created_at
		FROM artifact_link_old
		WHERE artifact_kind IN (
		  'bundle',
		  'analysis',
		  'signal',
		  'upstream_impact',
		  'control_plane_upgrade_candidate',
		  'release_delta',
		  'archive_manifest',
		  'ledger_export'
		);

		DROP TABLE artifact_link_old;
		",
	)?;

	Ok(())
}

pub(super) fn migrate_radar_review_statuses(connection: &Connection) -> Result<()> {
	let table_sql = connection
		.query_row(
			"
			SELECT sql
			FROM sqlite_master
			WHERE type = 'table' AND name = 'radar_review'
			",
			[],
			|row| row.get::<_, String>(0),
		)
		.optional()?;

	if table_sql.is_none() {
		return Ok(());
	};

	connection.execute_batch(
		"
		ALTER TABLE radar_review RENAME TO radar_review_old;

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

		INSERT OR REPLACE INTO radar_review (
		  repo,
		  subject_kind,
		  subject_id,
		  status,
		  reason,
		  confidence,
		  reviewed_at,
		  updated_at
		)
		SELECT
		  repo,
		  subject_kind,
		  subject_id,
		  status,
		  reason,
		  confidence,
		  reviewed_at,
		  updated_at
		FROM radar_review_old
		WHERE status IN (
		  'seen',
		  'skipped',
		  'watch',
		  'signal',
		  'control_plane',
		  'deprecated',
		  'archived'
		);

		DROP TABLE radar_review_old;
		",
	)?;

	Ok(())
}
