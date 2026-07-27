//! Bounded field, row, and storage enforcement for the disposable Radar ledger.

use rusqlite::Connection;

use crate::{
	LEDGER_MAX_BYTES, LEDGER_MAX_ROWS_PER_TABLE,
	prelude::{Result, eyre},
};

pub(super) const MAX_ARTIFACT_PATH_BYTES: usize = 4096;
pub(super) const MAX_EVIDENCE_TEXT_BYTES: usize = 2048;
pub(super) const MAX_IDENTIFIER_BYTES: usize = 256;
pub(super) const MAX_TITLE_BYTES: usize = 1024;
pub(super) const MAX_URL_BYTES: usize = 2048;
pub(super) const OVERSIZE_INCIDENT: &str = "RADAR_LEDGER_OVERSIZE";
const ROW_LIMIT_INCIDENT: &str = "RADAR_LEDGER_ROW_LIMIT";
const TABLES: &[(&str, &str)] = &[
	("upstream_commit", "last_seen_at"),
	("radar_review", "updated_at"),
	("artifact_link", "created_at"),
	("source_cache", "fetched_at"),
];

pub(super) fn validate_text(value: &str, label: &str, max_bytes: usize) -> Result<()> {
	if value.is_empty() || value.len() > max_bytes {
		eyre::bail!("{label} must contain 1 to {max_bytes} bytes");
	}

	Ok(())
}

pub(super) fn bounded_write(
	connection: &Connection,
	table: &str,
	timestamp: &str,
	operation: impl FnOnce() -> Result<()>,
) -> Result<()> {
	let owns_transaction = connection.is_autocommit();

	if owns_transaction {
		connection.execute_batch("BEGIN IMMEDIATE")?;
	} else {
		connection.execute_batch("SAVEPOINT radar_bounded_write")?;
	}

	let result = operation()
		.and_then(|()| prune_table(connection, table, timestamp, LEDGER_MAX_ROWS_PER_TABLE))
		.and_then(|()| validate_storage_bytes(connection, LEDGER_MAX_BYTES));

	if owns_transaction {
		match result {
			Ok(()) => connection.execute_batch("COMMIT")?,
			Err(error) => {
				let _ = connection.execute_batch("ROLLBACK");

				return Err(error);
			},
		}
	} else {
		match result {
			Ok(()) => connection.execute_batch("RELEASE radar_bounded_write")?,
			Err(error) => {
				let _ = connection
					.execute_batch("ROLLBACK TO radar_bounded_write; RELEASE radar_bounded_write");

				return Err(error);
			},
		}
	}

	Ok(())
}

pub(super) fn validate_ledger_bounds(connection: &Connection) -> Result<()> {
	for (table, _) in TABLES {
		let rows: i64 =
			connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))?;
		if rows > LEDGER_MAX_ROWS_PER_TABLE as i64 {
			eyre::bail!("{ROW_LIMIT_INCIDENT}: Radar ledger table exceeds the row limit");
		}
	}

	validate_storage_bytes(connection, LEDGER_MAX_BYTES)
}

fn prune_table(connection: &Connection, table: &str, timestamp: &str, limit: usize) -> Result<()> {
	if !TABLES.contains(&(table, timestamp)) {
		eyre::bail!("Radar ledger bound enforcement received an unknown table");
	}
	let limit =
		i64::try_from(limit).map_err(|_| eyre::eyre!("Radar ledger row limit is too large"))?;

	connection.execute(
		&format!(
			"DELETE FROM {table}
			 WHERE rowid NOT IN (
			   SELECT rowid FROM {table}
			   ORDER BY {timestamp} DESC, rowid DESC
			   LIMIT ?1
			 )"
		),
		[limit],
	)?;

	Ok(())
}

fn validate_storage_bytes(connection: &Connection, limit: u64) -> Result<()> {
	let page_count: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
	let page_size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
	let bytes = u64::try_from(page_count)
		.ok()
		.and_then(|count| u64::try_from(page_size).ok().map(|size| count.saturating_mul(size)))
		.ok_or_else(|| eyre::eyre!("Radar ledger returned invalid page metrics"))?;

	if bytes > limit {
		eyre::bail!(
			"{OVERSIZE_INCIDENT}: Radar ledger exceeds the byte limit after oldest-first \
			 retention"
		);
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn nested_bounded_write_rolls_back_partial_operation_on_retention_failure() {
		let connection = Connection::open_in_memory().expect("in-memory ledger should open");

		connection
			.execute_batch(
				"
				CREATE TABLE source_cache (
				  url TEXT PRIMARY KEY,
				  fetched_at TEXT NOT NULL
				);
				BEGIN IMMEDIATE;
				",
			)
			.expect("outer ledger transaction should start");
		let error = bounded_write(&connection, "unknown_table", "fetched_at", || {
			connection.execute(
				"
				INSERT INTO source_cache (url, fetched_at)
				VALUES ('https://example.com/source', '2026-07-27T00:00:00Z')
				",
				[],
			)?;

			Ok(())
		})
		.expect_err("retention failure must reject the nested write");
		let rows: i64 = connection
			.query_row("SELECT COUNT(*) FROM source_cache", [], |row| row.get(0))
			.expect("source row count should be readable");

		assert!(error.to_string().contains("unknown table"));
		assert_eq!(rows, 0);
		connection.execute_batch("COMMIT").expect("outer transaction should remain usable");
	}
}
