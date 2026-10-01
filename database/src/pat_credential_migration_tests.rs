use rusqlite::{self, Connection};
use tempfile::TempDir;

use crate::migrations::{self, APPLICATION_ID, MIGRATIONS};

fn version_51_fixture() -> (TempDir, Connection) {
	let directory = tempfile::tempdir().unwrap();
	let connection = Connection::open(directory.path().join("upgrade.sqlite3")).unwrap();

	migrations::configure(&connection).unwrap();

	for migration in MIGRATIONS.iter().filter(|migration| migration.version < 52) {
		connection.execute_batch(migration.sql).unwrap();
		connection
			.execute(
				"INSERT INTO schema_migrations VALUES(?1,?2,?3,1)",
				rusqlite::params![
					migration.version,
					migration.name,
					migrations::migration_digest(migration.sql)
				],
			)
			.unwrap();
	}

	connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
	connection.pragma_update(None, "user_version", 51).unwrap();
	connection.execute_batch(" 
INSERT INTO account_identities VALUES ('20000000-0000-4000-8000-000000000039',1);
INSERT INTO accounts(account_id,display_label,enabled,state,revision,provider,provider_account_id,created_at_micros,updated_at_micros)
VALUES('20000000-0000-4000-8000-000000000039','PAT fixture',1,'available',1,'chatgpt','provider',1,1);
INSERT INTO account_operations(operation_id,account_id,kind,phase,provider,provider_account_id,requested_display_label,requested_enabled,created_at_micros,updated_at_micros)
VALUES('30000000-0000-4000-8000-000000000039','20000000-0000-4000-8000-000000000039','enroll','prepared','chatgpt','provider','PAT fixture',1,1,1);
INSERT INTO account_credentials VALUES('20000000-0000-4000-8000-000000000039',1,1,printf('%064d',0),'30000000-0000-4000-8000-000000000039','chatgpt','provider',x'010203',1);
INSERT INTO process_execution_epochs VALUES('40000000-0000-4000-8000-000000000039',printf('%064d',0),1);
INSERT INTO process_generations(generation_id,account_id,execution_epoch_id,runner_identity,intended_boot_id,control_kind,isolation_kind,account_revision,credential_schema_version,credential_version,credential_fingerprint,credential_writer_operation_id,provider,provider_account_id,refresh_callback_profile_sha256,state,revision,created_at_micros,updated_at_micros)
VALUES('50000000-0000-4000-8000-000000000039','20000000-0000-4000-8000-000000000039','40000000-0000-4000-8000-000000000039','runner','boot','stdio_only_best_effort_eof','session',1,1,1,printf('%064d',0),'30000000-0000-4000-8000-000000000039','chatgpt','provider',printf('%064d',0),'starting',1,1,1);
INSERT INTO agent_voice_observed_turns VALUES('50000000-0000-4000-8000-000000000039','thread','turn');
").unwrap();

	(directory, connection)
}

#[test]
fn pat_migration_preserves_payloads_and_dependent_process_rows() {
	let (_directory, mut connection) = version_51_fixture();

	migrations::migrate(&mut connection).unwrap();

	let old: (i64, Vec<u8>) = connection
		.query_row("SELECT schema_version,payload FROM account_credentials", [], |row| {
			Ok((row.get(0)?, row.get(1)?))
		})
		.unwrap();

	assert_eq!(old, (1, vec![1, 2, 3]));
	assert_eq!(
		connection
			.query_row(
				"SELECT turn_id FROM agent_voice_observed_turns JOIN process_generations USING(generation_id)",
				[],
				|row| row.get::<_, String>(0)
			)
			.unwrap(),
		"turn"
	);

	connection.execute("UPDATE account_credentials SET schema_version=2", []).unwrap();
	connection.execute("UPDATE process_generations SET credential_schema_version=2", []).unwrap();

	assert!(connection.execute("UPDATE account_credentials SET schema_version=3", []).is_err());
	assert!(
		connection
			.execute("UPDATE process_generations SET credential_schema_version=3", [])
			.is_err()
	);

	migrations::migrate(&mut connection).unwrap();

	assert!(connection.execute("DELETE FROM process_generations", []).is_err());
	assert_eq!(
		connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0)).unwrap(),
		1
	);
}

#[test]
fn pat_migration_rolls_back_and_restores_foreign_keys_on_invalid_source() {
	let (_directory, mut connection) = version_51_fixture();

	connection.pragma_update(None, "foreign_keys", false).unwrap();
	connection.execute("DELETE FROM process_execution_epochs", []).unwrap();
	connection.pragma_update(None, "foreign_keys", true).unwrap();

	assert!(migrations::migrate(&mut connection).is_err());
	assert_eq!(
		connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0)).unwrap(),
		1
	);
	assert_eq!(
		connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)).unwrap(),
		51
	);
	assert_eq!(
		connection
			.query_row("SELECT payload FROM account_credentials", [], |row| row
				.get::<_, Vec<u8>>(0))
			.unwrap(),
		vec![1, 2, 3]
	);
}
