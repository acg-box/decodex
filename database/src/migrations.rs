use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

use crate::{DatabaseError, error::sqlite_error};

pub(crate) const APPLICATION_ID: i64 = 0x4443_5831;
const CURRENT_SCHEMA_VERSION: i64 = 33;

#[derive(Clone, Copy)]
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
	Migration {
		version: 13,
		name: "chief_work",
		sql: include_str!("../migrations/0013_chief_work.sql"),
	},
	Migration {
		version: 14,
		name: "optional_quota_window",
		sql: include_str!("../migrations/0014_optional_quota_window.sql"),
	},
	Migration {
		version: 15,
		name: "chief_live_output",
		sql: include_str!("../migrations/0015_chief_live_output.sql"),
	},
	Migration {
		version: 16,
		name: "chief_managers",
		sql: include_str!("../migrations/0016_chief_managers.sql"),
	},
	Migration {
		version: 17,
		name: "chief_tool_versions",
		sql: include_str!("../migrations/0017_chief_tool_versions.sql"),
	},
	Migration {
		version: 18,
		name: "chief_usage",
		sql: include_str!("../migrations/0018_chief_usage.sql"),
	},
	Migration {
		version: 19,
		name: "process_kernel_recovery",
		sql: include_str!("../migrations/0019_process_kernel_recovery.sql"),
	},
	Migration {
		version: 20,
		name: "chief_turn_usage",
		sql: include_str!("../migrations/0020_chief_turn_usage.sql"),
	},
	Migration {
		version: 21,
		name: "chief_account_rotation",
		sql: include_str!("../migrations/0021_chief_account_rotation.sql"),
	},
	Migration {
		version: 22,
		name: "chief_voice_calls",
		sql: include_str!("../migrations/0022_chief_voice_calls.sql"),
	},
	Migration {
		version: 23,
		name: "chief_observation_indexes",
		sql: include_str!("../migrations/0015_chief_observation_indexes.sql"),
	},
	Migration {
		version: 24,
		name: "chief_capacity_retry",
		sql: include_str!("../migrations/0016_chief_capacity_retry.sql"),
	},
	Migration {
		version: 25,
		name: "chief_async_questions",
		sql: include_str!("../migrations/0025_chief_async_questions.sql"),
	},
	Migration {
		version: 26,
		name: "chief_misalignment",
		sql: include_str!("../migrations/0026_chief_misalignment.sql"),
	},
	Migration {
		version: 27,
		name: "chief_guardian_reviews",
		sql: include_str!("../migrations/0027_chief_guardian_reviews.sql"),
	},
	Migration {
		version: 28,
		name: "conversation_service_tier",
		sql: include_str!("../migrations/0028_conversation_service_tier.sql"),
	},
	Migration {
		version: 29,
		name: "reset_card_operations",
		sql: include_str!("../migrations/0029_reset_card_operations.sql"),
	},
	Migration {
		version: 30,
		name: "quota_activation",
		sql: include_str!("../migrations/0030_quota_activation.sql"),
	},
	Migration {
		version: 31,
		name: "quota_activation_observation",
		sql: include_str!("../migrations/0031_quota_activation_observation.sql"),
	},
	Migration {
		version: 32,
		name: "account_usage_observation",
		sql: include_str!("../migrations/0032_account_usage_observation.sql"),
	},
	Migration {
		version: 33,
		name: "account_usage_conditions",
		sql: include_str!("../migrations/0033_account_usage_conditions.sql"),
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
	for migration in migration_plan(connection)? {
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

// Both deployed lineages retain their exact applied names and SQL digests.
// Only unapplied migrations receive the next available version in that lineage.
fn migration_plan(connection: &Connection) -> Result<Vec<Migration>, DatabaseError> {
	if !migration_table_exists(connection)? {
		return Ok(MIGRATIONS.to_vec());
	}
	let name: Option<String> = connection
		.query_row("SELECT name FROM schema_migrations WHERE version=15", [], |row| row.get(0))
		.optional()
		.map_err(sqlite_error)?;
	if name.as_deref() != Some("chief_observation_indexes") {
		return Ok(MIGRATIONS.to_vec());
	}
	// Versions 24, 25 and 26 shipped with a moving slice before the eight older
	// entries. Preserve whichever exact historical order this database applied,
	// then append. Never renumber or rewrite an applied migration to fit a new plan.
	let applied = connection
		.prepare("SELECT version,name FROM schema_migrations ORDER BY version")
		.map_err(sqlite_error)?
		.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
		.map_err(sqlite_error)?
		.collect::<Result<Vec<_>, _>>()
		.map_err(sqlite_error)?;
	for historical_end in 24..=26 {
		let plan: Vec<Migration> = MIGRATIONS[..14]
			.iter()
			.chain(MIGRATIONS[22..historical_end].iter())
			.chain(MIGRATIONS[14..22].iter())
			.chain(MIGRATIONS[historical_end..].iter())
			.enumerate()
			.map(|(index, migration)| Migration { version: index as i64 + 1, ..*migration })
			.collect();
		if applied.iter().enumerate().all(|(index, (version, name))| {
			plan.get(index).is_some_and(|m| *version == m.version && name == m.name)
		}) {
			return Ok(plan);
		}
	}
	Err(DatabaseError::Incompatible)
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
	let plan = migration_plan(connection)?;
	for (index, (version, name, digest)) in applied.iter().enumerate() {
		let expected = plan.get(index).ok_or(DatabaseError::Incompatible)?;
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn activation_upgrade_preserves_existing_preference_and_defaults_on() {
		let directory = tempfile::tempdir().expect("test directory");
		let mut connection =
			Connection::open(directory.path().join("upgrade.sqlite3")).expect("database");
		configure(&connection).expect("configure");
		for migration in &MIGRATIONS[..29] {
			connection.execute_batch(migration.sql).expect("previous migration");
			connection
				.execute(
					"INSERT INTO schema_migrations VALUES (?1,?2,?3,1)",
					params![migration.version, migration.name, migration_digest(migration.sql)],
				)
				.expect("ledger");
		}
		connection
			.execute("UPDATE desktop_settings SET show_in_menu_bar=0,revision=7", [])
			.expect("preference");
		connection.pragma_update(None, "application_id", APPLICATION_ID).expect("identity");
		connection.pragma_update(None, "user_version", 29).expect("version");
		migrate(&mut connection).expect("upgrade");
		let settings: (bool, bool, i64) = connection
			.query_row(
				"SELECT show_in_menu_bar,auto_activate_quota,revision FROM desktop_settings",
				[],
				|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
			)
			.expect("settings");
		assert_eq!(settings, (false, true, 7));
		verify(&connection).expect("schema parity");
		migrate(&mut connection).expect("idempotent upgrade");
	}

	#[test]
	fn reset_card_upgrade_preserves_existing_schema_and_initializes_an_empty_ledger() {
		let directory = tempfile::tempdir().expect("isolated migration root");
		let mut connection =
			Connection::open(directory.path().join("reset.sqlite3")).expect("isolated database");
		configure(&connection).expect("SQLite settings");
		for migration in &MIGRATIONS[..28] {
			connection.execute_batch(migration.sql).expect("prior migration");
			connection
				.execute(
					"INSERT INTO schema_migrations(version,name,sha256,applied_at_micros) VALUES(?1,?2,?3,1)",
					params![migration.version, migration.name, migration_digest(migration.sql)],
				)
				.expect("prior receipt");
		}
		connection
			.pragma_update(None, "application_id", APPLICATION_ID)
			.expect("application identity");
		connection.pragma_update(None, "user_version", 28).expect("prior version");
		connection
			.execute("UPDATE desktop_settings SET show_in_menu_bar=0, revision=17", [])
			.expect("user preference");
		let before = schema_inventory(&connection).expect("prior schema");
		migrate(&mut connection).expect("upgrade");
		verify(&connection).expect("upgraded schema");
		let after = schema_inventory(&connection).expect("current schema");
		assert!(
			before
				.iter()
				.filter(|entry| entry.2 != "desktop_settings")
				.all(|entry| after.contains(entry))
		);
		let count: i64 = connection
			.query_row("SELECT COUNT(*) FROM reset_card_operations", [], |row| row.get(0))
			.expect("empty ledger");
		assert_eq!(count, 0);
		let revision: i64 = connection
			.query_row("SELECT revision FROM desktop_settings", [], |row| row.get(0))
			.expect("preserved preference");
		assert_eq!(revision, 17);
	}

	#[test]
	fn service_tier_upgrade_adds_nullable_column_without_rewriting_prior_schema() {
		let directory = tempfile::tempdir().unwrap();
		let mut connection = Connection::open(directory.path().join("tiers.sqlite3")).unwrap();
		configure(&connection).unwrap();
		for migration in &MIGRATIONS[..27] {
			connection.execute_batch(migration.sql).unwrap();
			connection
				.execute(
					"INSERT INTO schema_migrations(version,name,sha256,applied_at_micros) VALUES(?1,?2,?3,1)",
					params![migration.version, migration.name, migration_digest(migration.sql)],
				)
				.unwrap();
		}
		connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
		connection.pragma_update(None, "user_version", 27).unwrap();
		for (id, fast) in [
			("10000000-0000-4000-8000-000000000001", 0),
			("10000000-0000-4000-8000-000000000002", 1),
		] {
			connection.execute("INSERT INTO conversations(conversation_id,kind,state,title,revision,created_at_micros,updated_at_micros) VALUES(?1,'ordinary_task','active','Legacy',1,1,1)",[id]).unwrap();
			connection.execute("INSERT INTO quick_task_requests(conversation_id,operation_key,correlation_id,initial_turn_id,message,working_directory,created_at_micros,model,reasoning_effort,fast) VALUES(?1,?1,?1,?1,'Keep original','/tmp',1,'model','high',?2)",params![id,fast]).unwrap();
		}
		let before = schema_inventory(&connection).unwrap();
		migrate(&mut connection).unwrap();
		verify(&connection).unwrap();
		let after = schema_inventory(&connection).unwrap();
		assert!(
			before
				.iter()
				.filter(|entry| !["quick_task_requests", "desktop_settings"]
					.contains(&entry.2.as_str()))
				.all(|entry| after.contains(entry))
		);
		let field:(String,i64,Option<String>) = connection.query_row("SELECT type,\"notnull\",dflt_value FROM pragma_table_info('quick_task_requests') WHERE name='service_tier'",[],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
		assert_eq!(field, ("TEXT".into(), 0, None));
		let saved = connection
			.prepare("SELECT fast,service_tier,message FROM quick_task_requests ORDER BY fast")
			.unwrap()
			.query_map([], |r| {
				Ok((r.get::<_, bool>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?))
			})
			.unwrap()
			.collect::<Result<Vec<_>, _>>()
			.unwrap();
		assert_eq!(
			saved,
			vec![(false, None, "Keep original".into()), (true, None, "Keep original".into())]
		);
		migrate(&mut connection).unwrap();
		verify(&connection).unwrap();
	}

	#[test]
	fn both_chief_migration_lineages_preserve_history_and_converge() {
		for (upstream, historical_end) in [(false, 26), (true, 24), (true, 25), (true, 26)] {
			let plan: Vec<Migration> = if upstream {
				MIGRATIONS[..14]
					.iter()
					.chain(MIGRATIONS[22..historical_end].iter())
					.chain(MIGRATIONS[14..22].iter())
					.chain(MIGRATIONS[historical_end..].iter())
					.enumerate()
					.map(|(index, m)| Migration { version: index as i64 + 1, ..*m })
					.collect()
			} else {
				MIGRATIONS.to_vec()
			};
			for version in 14..=26 {
				let directory = tempfile::tempdir().unwrap();
				let mut connection =
					Connection::open(directory.path().join("upgrade.sqlite3")).unwrap();
				configure(&connection).unwrap();
				for migration in &plan[..version] {
					connection.execute_batch(migration.sql).unwrap();
					connection
						.execute(
							"INSERT INTO schema_migrations VALUES (?1,?2,?3,1)",
							params![
								migration.version,
								migration.name,
								migration_digest(migration.sql)
							],
						)
						.unwrap();
				}
				connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
				connection.pragma_update(None, "user_version", version as i64).unwrap();
				let before: Vec<(i64, String, String)> = connection
					.prepare("SELECT version,name,sha256 FROM schema_migrations ORDER BY version")
					.unwrap()
					.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
					.unwrap()
					.collect::<Result<_, _>>()
					.unwrap();
				migrate(&mut connection).unwrap();
				let after: Vec<(i64, String, String)> = connection
					.prepare("SELECT version,name,sha256 FROM schema_migrations ORDER BY version")
					.unwrap()
					.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
					.unwrap()
					.collect::<Result<_, _>>()
					.unwrap();
				assert_eq!(before, after[..version]);
				verify(&connection).unwrap();
				migrate(&mut connection).unwrap();
			}
		}
	}

	#[test]
	fn kernel_recovery_migration_keeps_existing_evidence_bytes() {
		// Isolate row-copy behavior; fresh/upgrade tests verify the complete FK graph.
		let connection = Connection::open_in_memory().unwrap();
		connection.pragma_update(None, "foreign_keys", false).unwrap();
		connection.execute_batch(MIGRATIONS[0].sql).unwrap();
		connection.execute("INSERT INTO process_generation_death_evidence VALUES (?1,?2,'spawn_not_created','boot',NULL,NULL,NULL,NULL,NULL,?3,42)", params!["10000000-0000-4000-8000-000000000001", "10000000-0000-4000-8000-000000000002", "a".repeat(64)]).unwrap();
		let read = |c: &Connection| {
			c.query_row("SELECT evidence_id,generation_id,kind,observed_boot_id,witness_sha256,observed_at_micros FROM process_generation_death_evidence", [], |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?, row.get::<_,String>(3)?, row.get::<_,String>(4)?, row.get::<_,i64>(5)?))).unwrap()
		};
		let before = read(&connection);
		connection.execute_batch(MIGRATIONS[18].sql).unwrap();
		assert_eq!(read(&connection), before);
		connection
			.execute(
				"UPDATE process_generation_death_evidence SET kind='macos_kernel_confirmed_gone'",
				[],
			)
			.unwrap();
		assert!(
			connection
				.execute("UPDATE process_generation_death_evidence SET kind='timeout'", [])
				.is_err()
		);
	}

	#[test]
	fn chief_upgrade_preserves_exact_previous_schema_and_settings() {
		let directory = tempfile::tempdir().unwrap();
		let mut connection = Connection::open(directory.path().join("upgrade.sqlite3")).unwrap();
		configure(&connection).unwrap();
		for migration in &MIGRATIONS[..12] {
			connection.execute_batch(migration.sql).unwrap();
			connection.execute("INSERT INTO schema_migrations (version, name, sha256, applied_at_micros) VALUES (?1, ?2, ?3, 1)",
				params![migration.version, migration.name, migration_digest(migration.sql)]).unwrap();
		}
		connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
		connection.pragma_update(None, "user_version", 12).unwrap();
		connection
			.execute_batch(include_str!("../tests/fixtures/historical_program.sql"))
			.expect("historical Program fixture before upgrade");
		connection
			.execute("UPDATE desktop_settings SET show_in_menu_bar = 0, revision = 7", [])
			.unwrap();
		let original = schema_inventory(&connection).unwrap();
		migrate(&mut connection).unwrap();
		let upgraded = schema_inventory(&connection).unwrap();
		assert!(
			original
				.iter()
				.filter(|entry| ![
					"account_quota_facts",
					"desktop_settings",
					"process_generation_death_evidence",
					"quick_task_requests"
				]
				.contains(&entry.2.as_str()))
				.all(|entry| upgraded.contains(entry))
		);
		assert_eq!(
			connection
				.query_row("SELECT show_in_menu_bar, revision FROM desktop_settings", [], |row| Ok(
					(row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)
				))
				.unwrap(),
			(0, 7)
		);
		assert_eq!(applied_version(&connection).unwrap(), CURRENT_SCHEMA_VERSION);
		let retained: (String, String, String) = connection.query_row(
			"SELECT review.rationale, signal.predecessor_review_id, execution.conversation_id
			 FROM program_reviews AS review JOIN program_signals AS signal ON signal.predecessor_review_id = review.review_id
			 JOIN program_work_item_executions AS execution ON execution.work_item_id = review.work_item_id",
			[], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).expect("retained historical lineage");
		assert_eq!(
			retained,
			(
				"Historical rationale".into(),
				"10000000-0000-4000-8000-000000000009".into(),
				"10000000-0000-4000-8000-000000000010".into()
			)
		);
		migrate(&mut connection).unwrap();
		verify(&connection).unwrap();
	}

	#[test]
	fn async_question_upgrade_marks_existing_bound_threads_only() {
		let directory = tempfile::tempdir().unwrap();
		let mut connection = Connection::open(directory.path().join("upgrade.sqlite3")).unwrap();
		configure(&connection).unwrap();
		for migration in &MIGRATIONS[..24] {
			connection.execute_batch(migration.sql).unwrap();
			connection
				.execute(
					"INSERT INTO schema_migrations(version,name,sha256,applied_at_micros) VALUES(?1,?2,?3,1)",
					params![migration.version, migration.name, migration_digest(migration.sql)],
				)
				.unwrap();
		}
		connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
		connection.pragma_update(None, "user_version", 24).unwrap();
		connection.execute("INSERT INTO chief_work_items(id,kind,title,instructions,status,dispatch_state,codex_thread_id,created_at_micros,updated_at_micros) VALUES('bound','goal','Bound','Keep','open','idle','native',1,1),('unbound','goal','Unbound','Keep','open','idle',NULL,1,1)",[]).unwrap();
		migrate(&mut connection).unwrap();
		let rows = connection
			.prepare("SELECT work_id,thread_id FROM chief_async_recovery")
			.unwrap()
			.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
			.unwrap()
			.collect::<Result<Vec<_>, _>>()
			.unwrap();
		assert_eq!(rows, vec![("bound".into(), "native".into())]);
		migrate(&mut connection).unwrap();
		assert_eq!(
			connection
				.query_row("SELECT count(*) FROM chief_async_recovery", [], |row| row
					.get::<_, i64>(0))
				.unwrap(),
			1
		);
	}
	#[test]
	fn capacity_retry_upgrade_preserves_version_fourteen_and_fifteen_events() {
		for version in [14, 15] {
			let directory = tempfile::tempdir().unwrap();
			let mut connection =
				Connection::open(directory.path().join("upgrade.sqlite3")).unwrap();
			configure(&connection).unwrap();
			for migration in &MIGRATIONS[..version] {
				connection.execute_batch(migration.sql).unwrap();
				connection
					.execute(
						"INSERT INTO schema_migrations (version,name,sha256,applied_at_micros) VALUES (?1,?2,?3,1)",
						params![migration.version, migration.name, migration_digest(migration.sql)],
					)
					.unwrap();
			}
			connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
			connection.pragma_update(None, "user_version", version as i64).unwrap();
			connection.execute("INSERT INTO chief_work_items (id,kind,title,instructions,status,dispatch_state,created_at_micros,updated_at_micros) VALUES ('root','goal','Keep this goal','Keep these instructions','open','idle',1,1)", []).unwrap();
			connection.execute("INSERT INTO chief_inbox_events (source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES ('input','root','user_message','{\"text\":\"Keep this message\"}',1)", []).unwrap();
			migrate(&mut connection).unwrap();
			let event: (String, Option<String>) = connection
				.query_row(
					"SELECT payload,disposition FROM chief_inbox_events WHERE source_event_id='input'",
					[],
					|row| Ok((row.get(0)?, row.get(1)?)),
				)
				.unwrap();
			assert_eq!(event, ("{\"text\":\"Keep this message\"}".into(), None));
			let indexes: i64 = connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='index' AND name IN ('chief_inbox_work_history','chief_inbox_work_kind')", [], |row| row.get(0)).unwrap();
			assert_eq!(indexes, 2);
			migrate(&mut connection).unwrap();
			verify(&connection).unwrap();
		}
	}

	#[test]
	fn optional_quota_upgrade_preserves_version_thirteen_facts_and_errors() {
		let directory = tempfile::tempdir().expect("temporary database");
		let mut connection =
			Connection::open(directory.path().join("upgrade.sqlite3")).expect("open");
		configure(&connection).expect("configure");
		for migration in &MIGRATIONS[..13] {
			connection.execute_batch(migration.sql).expect("historical migration");
			connection
				.execute(
					"INSERT INTO schema_migrations (version,name,sha256,applied_at_micros) VALUES (?1,?2,?3,1)",
					params![migration.version, migration.name, migration_digest(migration.sql)],
				)
				.expect("record history");
		}
		connection.pragma_update(None, "application_id", APPLICATION_ID).expect("identity");
		connection.pragma_update(None, "user_version", 13).expect("version");
		let account = "10000000-0000-4000-8000-000000000001";
		connection
			.execute("INSERT INTO account_identities VALUES (?1,1)", [account])
			.expect("identity row");
		connection
			.execute("INSERT INTO account_quota_facts VALUES (?1,300,100,900,NULL,10)", [account])
			.expect("fact row");
		connection
			.execute(
				"INSERT INTO account_quota_facts VALUES (?1,10080,NULL,NULL,'unsupported_window',11)",
				[account],
			)
			.expect("error row");
		migrate(&mut connection).expect("forward migration");
		type QuotaRow = (i64, Option<i64>, Option<i64>, Option<String>, i64, i64);
		let values: Vec<QuotaRow> = connection.prepare("SELECT duration_minutes,used_percent,resets_at_micros,error_code,observed_at_micros,not_applicable FROM account_quota_facts ORDER BY duration_minutes").expect("query").query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).expect("rows").collect::<Result<_,_>>().expect("values");
		assert_eq!(
			values,
			vec![
				(300, Some(100), Some(900), None, 10, 0),
				(10080, None, None, Some("unsupported_window".into()), 11, 0)
			]
		);
		assert!(
			connection
				.execute(
					"UPDATE account_quota_facts SET not_applicable=1 WHERE duration_minutes=10080",
					[]
				)
				.is_err()
		);
		assert!(
			connection
				.execute(
					"UPDATE account_quota_facts SET not_applicable=1 WHERE duration_minutes=300",
					[]
				)
				.is_err()
		);
		verify(&connection).expect("canonical schema parity");
		migrate(&mut connection).expect("idempotent reopen");
	}
}
