use rusqlite::{self, Connection};

use crate::migrations::{self, APPLICATION_ID, MIGRATIONS};

#[test]
fn voice_precaution_upgrade_preserves_unknown_legacy_cause() {
	let directory = tempfile::tempdir().unwrap();
	let mut connection = Connection::open(directory.path().join("voice.sqlite3")).unwrap();

	migrations::configure(&connection).unwrap();

	for migration in &MIGRATIONS[..1] {
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
	connection.pragma_update(None, "user_version", 48).unwrap();
	connection.execute("INSERT INTO agent_work_items(id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES('work','goal','Goal','Keep input','open',1,1)",[]).unwrap();
	connection
		.execute("INSERT INTO agent_misalignment VALUES('work','thread','turn','{}',1)", [])
		.unwrap();

	migrations::migrate(&mut connection).unwrap();
	migrations::verify(&connection).unwrap();

	let saved: (String, String, String, i64, bool) = connection
		.query_row(
			"SELECT thread_id,turn_id,details_json,created_at_micros,retired_voice FROM agent_misalignment",
			[],
			|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
		)
		.unwrap();

	assert_eq!(saved, ("thread".into(), "turn".into(), "{}".into(), 1, true));
	assert!(connection.execute("UPDATE agent_misalignment SET retired_voice=2", []).is_err());

	migrations::migrate(&mut connection).unwrap();
	migrations::verify(&connection).unwrap();
}
