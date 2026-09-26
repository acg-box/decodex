use super::{APPLICATION_ID, MIGRATIONS, configure, migrate, migration_digest, verify};
use rusqlite::{Connection, params};

type LegacyRequest =
	(String, String, String, Option<String>, Option<String>, Option<String>, Option<i64>, bool);

#[test]
fn initial_source_upgrade_preserves_requests_and_enforces_paired_identity() {
	let directory = tempfile::tempdir().expect("migration directory");
	let mut connection =
		Connection::open(directory.path().join("upgrade.sqlite3")).expect("database");
	configure(&connection).expect("configure");
	for migration in &MIGRATIONS[..45] {
		connection.execute_batch(migration.sql).expect("old migration");
		connection
			.execute(
				"INSERT INTO schema_migrations(version,name,sha256,applied_at_micros) VALUES(?1,?2,?3,1)",
				params![migration.version, migration.name, migration_digest(migration.sql)],
			)
			.expect("migration receipt");
	}
	connection.pragma_update(None, "application_id", APPLICATION_ID).expect("application");
	connection.pragma_update(None, "user_version", 45).expect("version");
	connection.execute_batch(
        "INSERT INTO conversations(conversation_id,kind,state,title,revision,created_at_micros,updated_at_micros)
         VALUES('50000000-0000-4000-8000-000000000001','ordinary_task','active','Original',7,1,2);
         INSERT INTO quick_task_requests(conversation_id,operation_key,correlation_id,initial_turn_id,message,working_directory,created_at_micros,model,reasoning_effort,service_tier)
         VALUES('50000000-0000-4000-8000-000000000001','original','original','60000000-0000-4000-8000-000000000001','Keep original input','/saved/directory',1,'native-model',NULL,NULL);"
    ).expect("legacy request");
	migrate(&mut connection).expect("upgrade");
	verify(&connection).expect("verify upgrade");
	let retained: LegacyRequest = connection.query_row(
        "SELECT message,working_directory,model,reasoning_effort,service_tier,model_source_account_id,model_source_account_revision,model_source_review_required FROM quick_task_requests",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?))
    ).expect("upgraded request");
	assert_eq!(
		retained,
		(
			"Keep original input".into(),
			"/saved/directory".into(),
			"native-model".into(),
			None,
			None,
			None,
			None,
			false
		)
	);
	for sql in [
		"UPDATE quick_task_requests SET model_source_account_id='account'",
		"UPDATE quick_task_requests SET model_source_account_revision=1",
		"UPDATE quick_task_requests SET model_source_account_id='account', model_source_account_revision=0",
		"UPDATE quick_task_requests SET model_source_account_id='account', model_source_account_revision=-1",
		"UPDATE quick_task_requests SET model_source_review_required=2",
	] {
		assert!(connection.execute(sql, []).is_err(), "invalid source must be rejected: {sql}");
	}
	connection.execute("UPDATE quick_task_requests SET model_source_account_id='30000000-0000-4000-8000-000000000003',model_source_account_revision=9,model_source_review_required=1", []).expect("paired source");
	migrate(&mut connection).expect("idempotent migration");
	let state: (i64, bool) = connection
		.query_row(
			"SELECT model_source_account_revision,model_source_review_required FROM quick_task_requests",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.expect("source retained");
	assert_eq!(state, (9, true));
}
