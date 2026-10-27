//! Initialize and verify the one current product schema. Older schemas are not upgraded.
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior};

use crate::{DatabaseError, error::sqlite_error};

pub(crate) const APPLICATION_ID: i64 = 0x4443_5831;
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 60;

const SCHEMA: &str = include_str!("../schema.sql");

pub(crate) fn configure(connection: &Connection) -> Result<(), DatabaseError> {
	connection.busy_timeout(Duration::from_secs(5)).map_err(sqlite_error)?;
	connection
		.execute_batch(
			"PRAGMA foreign_keys = ON;
			 PRAGMA trusted_schema = OFF;
			 PRAGMA temp_store = MEMORY;
			 PRAGMA synchronous = FULL;
			 PRAGMA wal_autocheckpoint = 1000;",
		)
		.map_err(sqlite_error)?;

	let journal_mode: String = connection
		.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
		.map_err(sqlite_error)?;

	if !journal_mode.eq_ignore_ascii_case("wal") {
		return Err(DatabaseError::Incompatible);
	}

	Ok(())
}

pub(crate) fn initialize(connection: &mut Connection) -> Result<(), DatabaseError> {
	if user_table_count(connection)? == 0 {
		let application: i64 = connection
			.query_row("PRAGMA application_id", [], |row| row.get(0))
			.map_err(sqlite_error)?;

		if application != 0 && application != APPLICATION_ID {
			return Err(DatabaseError::Incompatible);
		}

		let tx = connection
			.transaction_with_behavior(TransactionBehavior::Immediate)
			.map_err(sqlite_error)?;

		tx.execute_batch(SCHEMA).map_err(sqlite_error)?;
		tx.pragma_update(None, "application_id", APPLICATION_ID).map_err(sqlite_error)?;
		tx.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION).map_err(sqlite_error)?;
		tx.commit().map_err(sqlite_error)?;
	}

	verify(connection)
}

pub(crate) fn verify(connection: &Connection) -> Result<(), DatabaseError> {
	if user_table_count(connection)? == 0
		|| connection
			.query_row("PRAGMA application_id", [], |row| row.get::<_, i64>(0))
			.map_err(sqlite_error)?
			!= APPLICATION_ID
	{
		return Err(DatabaseError::Incompatible);
	}
	if connection
		.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
		.map_err(sqlite_error)?
		!= CURRENT_SCHEMA_VERSION
	{
		return Err(DatabaseError::Incompatible);
	}

	let foreign_keys: i64 =
		connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0)).map_err(sqlite_error)?;
	let journal_mode: String =
		connection.query_row("PRAGMA journal_mode", [], |row| row.get(0)).map_err(sqlite_error)?;
	let synchronous: i64 =
		connection.query_row("PRAGMA synchronous", [], |row| row.get(0)).map_err(sqlite_error)?;

	if foreign_keys != 1 || !journal_mode.eq_ignore_ascii_case("wal") || synchronous != 2 {
		return Err(DatabaseError::Incompatible);
	}

	let quick_check: String = connection
		.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
		.map_err(sqlite_error)?;

	if quick_check != "ok" {
		return Err(DatabaseError::Corrupt);
	}

	let foreign_key_violation = connection
		.query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
		.optional()
		.map_err(sqlite_error)?;

	if foreign_key_violation.is_some() || schema_inventory(connection)? != expected_inventory()? {
		return Err(DatabaseError::Incompatible);
	}

	Ok(())
}

fn user_table_count(connection: &Connection) -> Result<i64, DatabaseError> {
	connection
		.query_row(
			"SELECT COUNT(*) FROM sqlite_schema
			 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
			[],
			|row| row.get(0),
		)
		.map_err(sqlite_error)
}

fn expected_inventory() -> Result<Vec<(String, String, String, String)>, DatabaseError> {
	let connection = Connection::open_in_memory().map_err(sqlite_error)?;

	connection.execute_batch(SCHEMA).map_err(sqlite_error)?;

	schema_inventory(&connection)
}

fn schema_inventory(
	connection: &Connection,
) -> Result<Vec<(String, String, String, String)>, DatabaseError> {
	let mut statement = connection
		.prepare(
			"SELECT type, name, tbl_name, sql
			 FROM sqlite_schema
			 WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL
			 ORDER BY type, name",
		)
		.map_err(sqlite_error)?;
	let rows = statement
		.query_map([], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, String>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, String>(3)?,
			))
		})
		.map_err(sqlite_error)?;

	rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)
}
