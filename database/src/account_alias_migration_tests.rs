#[test]
fn aliases_upgrade_once_without_changing_account_authority() {
	use super::{APPLICATION_ID, MIGRATIONS, configure, migrate, migration_digest};

	use rusqlite::{Connection, params};

	let directory = tempfile::tempdir().unwrap();
	let mut connection = Connection::open(directory.path().join("upgrade.sqlite3")).unwrap();

	configure(&connection).unwrap();

	for migration in MIGRATIONS.iter().filter(|migration| migration.version < 50) {
		connection.execute_batch(migration.sql).unwrap();
		connection
			.execute(
				"INSERT INTO schema_migrations(version,name,sha256,applied_at_micros) VALUES(?1,?2,?3,1)",
				params![migration.version, migration.name, migration_digest(migration.sql)],
			)
			.unwrap();
	}

	connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
	connection.pragma_update(None, "user_version", 49).unwrap();

	for (index, provider) in ["provider-a", "provider-b"].iter().enumerate() {
		connection
			.execute(
				"INSERT INTO account_identities VALUES (?1,1)",
				[format!("20000000-0000-4000-8000-{index:012}")],
			)
			.unwrap();
		connection.execute("INSERT INTO accounts(account_id,display_label,enabled,state,revision,provider,provider_account_id,created_at_micros,updated_at_micros) VALUES(?1,'Val',1,'available',7,'chatgpt',?2,1,2)", params![format!("20000000-0000-4000-8000-{index:012}"),provider]).unwrap();
	}

	migrate(&mut connection).unwrap();

	let read = |connection: &Connection| {
		connection
			.prepare(
				"SELECT display_label,revision,enabled,updated_at_micros FROM accounts ORDER BY account_id",
			)
			.unwrap()
			.query_map([], |row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, i64>(1)?,
					row.get::<_, bool>(2)?,
					row.get::<_, i64>(3)?,
				))
			})
			.unwrap()
			.collect::<Result<Vec<_>, _>>()
			.unwrap()
	};
	let names = read(&connection);

	assert_ne!(names[0].0, names[1].0);
	assert!(
		names
			.iter()
			.all(|(_, revision, enabled, updated)| *revision == 7 && *enabled && *updated == 2)
	);

	migrate(&mut connection).unwrap();

	assert_eq!(names, read(&connection));
}
