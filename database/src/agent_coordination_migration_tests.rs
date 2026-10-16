use crate::migrations::{self, APPLICATION_ID, MIGRATIONS};
use rusqlite::{Connection, params};

#[test]
fn coordination_upgrade_preserves_history_and_only_keeps_outstanding_assignments() {
	let directory = tempfile::tempdir().unwrap();
	let mut c = Connection::open(directory.path().join("upgrade.sqlite3")).unwrap();
	migrations::configure(&c).unwrap();
	for migration in MIGRATIONS.iter().filter(|m| m.version <= 53) {
		c.execute_batch(migration.sql).unwrap();
		c.execute(
			"INSERT INTO schema_migrations VALUES(?1,?2,?3,1)",
			params![migration.version, migration.name, migrations::migration_digest(migration.sql)],
		)
		.unwrap();
	}
	c.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
	c.pragma_update(None, "user_version", 53).unwrap();
	c.execute("INSERT INTO agent_work_items(id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES('main','goal','Main','Keep context','open',1,1)",[]).unwrap();
	for (id, status, instruction) in [
		("done", "resolved", true),
		("waiting", "wait", true),
		("running", "open", true),
		("chat", "open", false),
	] {
		c.execute("INSERT INTO agent_work_items(id,parent_goal_id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES(?1,'main','goal',?1,'Keep context',?2,1,1)",params![id,status]).unwrap();
		c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?1,?2,'original history',1)",params![id,if instruction {"work_instruction"} else {"user_message"}]).unwrap();
	}
	c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('accepted','running','worker_turn_completed','accepted result',2,'resolved','Accepted',2)",[]).unwrap();
	migrations::migrate(&mut c).unwrap();
	let assignments: Vec<String> = c
		.prepare("SELECT work_id FROM agent_delegations ORDER BY work_id")
		.unwrap()
		.query_map([], |r| r.get(0))
		.unwrap()
		.collect::<Result<_, _>>()
		.unwrap();
	assert_eq!(assignments, ["waiting"]);
	assert_eq!(
		c.query_row(
			"SELECT count(*) FROM agent_inbox_events WHERE payload='original history'",
			[],
			|r| r.get::<_, i64>(0)
		)
		.unwrap(),
		4
	);
	assert_eq!(
		c.query_row("SELECT count(*) FROM agent_work_items", [], |r| r.get::<_, i64>(0)).unwrap(),
		5
	);
	migrations::migrate(&mut c).unwrap();
	assert_eq!(
		c.query_row("SELECT count(*) FROM agent_delegations", [], |r| r.get::<_, i64>(0)).unwrap(),
		1
	);
}
