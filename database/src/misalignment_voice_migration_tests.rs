use super::{
	APPLICATION_ID, MIGRATIONS, configure, migrate, migration_digest, schema_inventory, verify,
};
use rusqlite::{Connection, params};

#[test]
fn voice_precaution_upgrade_preserves_unknown_legacy_cause() {
	let directory = tempfile::tempdir().unwrap();
	let mut connection = Connection::open(directory.path().join("voice.sqlite3")).unwrap();
	configure(&connection).unwrap();
	for migration in &MIGRATIONS[..47] {
		connection.execute_batch(migration.sql).unwrap();
		connection
			.execute(
				"INSERT INTO schema_migrations(version,name,sha256,applied_at_micros) VALUES(?1,?2,?3,1)",
				params![migration.version, migration.name, migration_digest(migration.sql)],
			)
			.unwrap();
	}
	connection.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
	connection.pragma_update(None, "user_version", 47).unwrap();
	connection.execute("INSERT INTO chief_work_items(id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES('work','goal','Goal','Keep input','open',1,1)",[]).unwrap();
	connection
		.execute("INSERT INTO chief_misalignment VALUES('work','thread','turn','{}',1)", [])
		.unwrap();
	let original = schema_inventory(&connection).unwrap();
	migrate(&mut connection).unwrap();
	verify(&connection).unwrap();
	let saved: (String, String, String, i64, bool) = connection
		.query_row(
			"SELECT thread_id,turn_id,details_json,created_at_micros,retired_voice FROM chief_misalignment",
			[],
			|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
		)
		.unwrap();
	assert_eq!(
		schema_inventory(&connection)
			.unwrap()
			.into_iter()
			.filter(|r| r.2 != "chief_misalignment")
			.collect::<Vec<_>>(),
		original.into_iter().filter(|r| r.2 != "chief_misalignment").collect::<Vec<_>>()
	);
	assert_eq!(saved, ("thread".into(), "turn".into(), "{}".into(), 1, true));
	assert!(connection.execute("UPDATE chief_misalignment SET retired_voice=2", []).is_err());
	migrate(&mut connection).unwrap();
	verify(&connection).unwrap();
}
