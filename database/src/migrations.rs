use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

use crate::{DatabaseError, error::sqlite_error};

pub(crate) const APPLICATION_ID: i64 = 0x4443_5831;
const CURRENT_SCHEMA_VERSION: i64 = 12;

struct Migration {
	version: i64,
	name: &'static str,
	sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
	Migration {
		version: 1,
		name: "local_product",
		sql: include_str!("../migrations/0001_local_product.sql"),
	},
	Migration {
		version: 2,
		name: "nonempty_task_instructions",
		sql: include_str!("../migrations/0002_nonempty_task_instructions.sql"),
	},
	Migration {
		version: 3,
		name: "quick_task_execution_controls",
		sql: include_str!("../migrations/0003_quick_task_execution_controls.sql"),
	},
	Migration {
		version: 4,
		name: "context_pack_fallback",
		sql: include_str!("../migrations/0004_context_pack_fallback.sql"),
	},
	Migration {
		version: 5,
		name: "adaptive_factory_spine",
		sql: include_str!("../migrations/0005_adaptive_factory_spine.sql"),
	},
	Migration {
		version: 6,
		name: "repeatable_program_loop",
		sql: include_str!("../migrations/0006_repeatable_program_loop.sql"),
	},
	Migration {
		version: 7,
		name: "builtin_domain_pack_binding",
		sql: include_str!("../migrations/0007_builtin_domain_pack_binding.sql"),
	},
	Migration {
		version: 8,
		name: "account_reauthentication_takeover",
		sql: include_str!("../migrations/0008_account_reauthentication_takeover.sql"),
	},
	Migration {
		version: 9,
		name: "durable_account_route",
		sql: include_str!("../migrations/0009_durable_account_route.sql"),
	},
	Migration {
		version: 10,
		name: "pending_account_route_progress",
		sql: include_str!("../migrations/0010_pending_account_route_progress.sql"),
	},
	Migration {
		version: 11,
		name: "desktop_settings",
		sql: include_str!("../migrations/0011_desktop_settings.sql"),
	},
	Migration {
		version: 12,
		name: "terminal_account_route_upgrade",
		sql: include_str!("../migrations/0012_terminal_account_route_upgrade.sql"),
	},
];

pub(crate) fn configure(connection: &Connection) -> Result<(), DatabaseError> {
	connection.busy_timeout(std::time::Duration::from_secs(5)).map_err(sqlite_error)?;
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

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), DatabaseError> {
	let user_tables = user_table_count(connection)?;
	let application_id: i64 = connection
		.query_row("PRAGMA application_id", [], |row| row.get(0))
		.map_err(sqlite_error)?;
	if user_tables == 0 {
		if application_id != 0 && application_id != APPLICATION_ID {
			return Err(DatabaseError::Incompatible);
		}
	} else if application_id != APPLICATION_ID || !migration_table_exists(connection)? {
		return Err(DatabaseError::Incompatible);
	}

	verify_applied_migrations(connection)?;
	for migration in MIGRATIONS {
		if migration.version <= applied_version(connection)? {
			continue;
		}
		let digest = migration_digest(migration.sql);
		let now = now_micros()?;
		let transaction = connection
			.transaction_with_behavior(TransactionBehavior::Immediate)
			.map_err(sqlite_error)?;
		transaction.execute_batch(migration.sql).map_err(sqlite_error)?;
		transaction
			.execute(
				"INSERT INTO schema_migrations (version, name, sha256, applied_at_micros)
				 VALUES (?1, ?2, ?3, ?4)",
				params![migration.version, migration.name, digest, now],
			)
			.map_err(sqlite_error)?;
		transaction.pragma_update(None, "application_id", APPLICATION_ID).map_err(sqlite_error)?;
		transaction.pragma_update(None, "user_version", migration.version).map_err(sqlite_error)?;
		transaction.commit().map_err(sqlite_error)?;
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
	verify_applied_migrations(connection)?;
	if applied_version(connection)? != CURRENT_SCHEMA_VERSION {
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

fn verify_applied_migrations(connection: &Connection) -> Result<(), DatabaseError> {
	if !migration_table_exists(connection)? {
		return if user_table_count(connection)? == 0 {
			Ok(())
		} else {
			Err(DatabaseError::Incompatible)
		};
	}
	let mut statement = connection
		.prepare("SELECT version, name, sha256 FROM schema_migrations ORDER BY version")
		.map_err(sqlite_error)?;
	let rows = statement
		.query_map([], |row| {
			Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
		})
		.map_err(sqlite_error)?;
	let applied = rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)?;
	if applied.len() > MIGRATIONS.len() {
		return Err(DatabaseError::Incompatible);
	}
	for (index, (version, name, digest)) in applied.iter().enumerate() {
		let expected = MIGRATIONS.get(index).ok_or(DatabaseError::Incompatible)?;
		if *version != expected.version
			|| name != expected.name
			|| digest != &migration_digest(expected.sql)
		{
			return Err(DatabaseError::Incompatible);
		}
	}

	Ok(())
}

fn applied_version(connection: &Connection) -> Result<i64, DatabaseError> {
	if !migration_table_exists(connection)? {
		return Ok(0);
	}
	connection
		.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
		.map_err(sqlite_error)
}

fn migration_table_exists(connection: &Connection) -> Result<bool, DatabaseError> {
	connection
		.query_row(
			"SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'schema_migrations'",
			[],
			|row| row.get::<_, i64>(0),
		)
		.optional()
		.map(|value| value.is_some())
		.map_err(sqlite_error)
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

fn migration_digest(sql: &str) -> String {
	let mut digest = Sha256::new();
	digest.update(b"decodex-sqlite-migration-v1\0");
	digest.update(sql.as_bytes());
	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now_micros() -> Result<i64, DatabaseError> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|value| i64::try_from(value.as_micros()).ok())
		.filter(|value| *value > 0)
		.ok_or(DatabaseError::Unavailable)
}

fn expected_inventory() -> Result<Vec<(String, String, String, String)>, DatabaseError> {
	let connection = Connection::open_in_memory().map_err(sqlite_error)?;
	for migration in MIGRATIONS {
		connection.execute_batch(migration.sql).map_err(sqlite_error)?;
	}
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

#[cfg(test)]
pub(crate) fn expected_migration_digests() -> Vec<String> {
	MIGRATIONS.iter().map(|migration| migration_digest(migration.sql)).collect()
}
