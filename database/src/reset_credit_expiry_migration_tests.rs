use rusqlite::{self, Connection};

use crate::migrations::{self, APPLICATION_ID, MIGRATIONS};

#[test]
fn reset_credit_expiry_migration_preserves_attempts_and_account_exclusion() {
	let directory = tempfile::tempdir().unwrap();
	let mut connection = Connection::open(directory.path().join("upgrade.sqlite3")).unwrap();

	migrations::configure(&connection).unwrap();

	for migration in MIGRATIONS.iter().filter(|migration| migration.version < 51) {
		connection.execute_batch(migration.sql).unwrap();
		connection
			.execute(
				"INSERT INTO schema_migrations(version,name,sha256,applied_at_micros) VALUES(?1,?2,?3,1)",
				rusqlite::params![
					migration.version,
					migration.name,
					migrations::migration_digest(migration.sql)
				],
			)
			.unwrap();
	}

	connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
	connection.pragma_update(None, "user_version", 50).unwrap();
	connection.execute("INSERT INTO reset_card_operations VALUES ('old','account',7,100,200,'credit','sending',NULL,NULL)", []).unwrap();

	migrations::migrate(&mut connection).unwrap();

	let prior: (i64, i64, String, String) = connection.query_row("SELECT account_revision,expires_at,exact_credit_id,state FROM reset_card_operations WHERE idempotency_key='old'", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();

	assert_eq!(prior, (7, 200, "credit".into(), "sending".into()));
	assert!(connection.execute("INSERT INTO reset_card_operations VALUES ('duplicate','account',7,100,NULL,'credit2','prepared',NULL,NULL)", []).is_err());

	connection.execute("INSERT INTO reset_card_operations VALUES ('new','other',3,100,NULL,'credit2','prepared',NULL,NULL)", []).unwrap();

	assert!(connection.execute("INSERT INTO reset_card_operations VALUES ('invalid','third',3,100,99,'credit3','prepared',NULL,NULL)", []).is_err());

	migrations::migrate(&mut connection).unwrap();

	assert_eq!(
		connection
			.query_row("SELECT count(*) FROM reset_card_operations", [], |row| row.get::<_, i64>(0))
			.unwrap(),
		2
	);
}
