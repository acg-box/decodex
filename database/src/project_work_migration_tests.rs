use crate::migrations::{self, APPLICATION_ID, MIGRATIONS};
use rusqlite::{Connection, params};
#[test]
fn direct_migration_preserves_native_identity_directory_holds_and_completed_history() {
	let home = tempfile::tempdir().unwrap();
	let mut c = Connection::open(home.path().join("upgrade.db")).unwrap();
	migrations::configure(&c).unwrap();
	for m in MIGRATIONS.iter().filter(|m| m.version < 56) {
		c.execute_batch(m.sql).unwrap();
		c.execute(
			"INSERT INTO schema_migrations VALUES(?1,?2,?3,1)",
			params![m.version, m.name, migrations::migration_digest(m.sql)],
		)
		.unwrap();
	}
	c.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
	c.pragma_update(None, "user_version", 55).unwrap();
	c.execute_batch("INSERT INTO agent_work_items(id,parent_goal_id,kind,title,instructions,codex_thread_id,status,created_at_micros,updated_at_micros) VALUES('assistant',NULL,'goal','Main','Assist the user','native-assistant','open',1,1),('sage','assistant','goal','Sage','Plan the market','native-sage','open',1,1),('venue','sage','goal','Venue','Accessible indoor space',NULL,'user_decision',1,1),('budget','sage','task','Budget','Check costs','native-budget','resolved',1,1); INSERT INTO agent_managers VALUES('sage'); INSERT INTO workspaces VALUES('market','Market','/market'); INSERT INTO work_workspace VALUES('sage','market'),('venue','market'),('budget','market'); INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES('budget-instruction','budget','work_instruction','{\"text\":\"Check the 80 person budget\"}',1);").unwrap();
	migrations::migrate(&mut c).unwrap();
	migrations::verify(&c).unwrap();
	let ids: Vec<(String, Option<String>)> = c
		.prepare("SELECT id,codex_thread_id FROM agent_work_items ORDER BY id")
		.unwrap()
		.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
		.unwrap()
		.collect::<Result<_, _>>()
		.unwrap();
	assert_eq!(ids.len(), 4);
	assert_eq!(ids[3], ("venue".into(), None));
	assert_eq!(c.query_row("SELECT count(*) FROM agents", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
	assert_eq!(
		c.query_row("SELECT directory FROM projects WHERE id='market'", [], |r| r
			.get::<_, String>(0))
			.unwrap(),
		"/market"
	);
	assert_eq!(
		c.query_row("SELECT owner_agent_id,state FROM works WHERE id='venue'", [], |r| Ok((
			r.get::<_, String>(0)?,
			r.get::<_, String>(1)?
		)))
		.unwrap(),
		("sage".into(), "blocked".into())
	);
	assert_eq!(
		c.query_row("SELECT state FROM works WHERE id='budget'", [], |r| r.get::<_, String>(0))
			.unwrap(),
		"completed"
	);
	assert_eq!(
		c.query_row("SELECT goal FROM work_requirements WHERE work_id='budget'", [], |r| r
			.get::<_, String>(0))
			.unwrap(),
		"Check the 80 person budget"
	);
	assert_eq!(
		c.query_row(
			"SELECT count(*) FROM agent_inbox_events WHERE source_event_id='budget-instruction'",
			[],
			|r| r.get::<_, i64>(0)
		)
		.unwrap(),
		1
	);
}

#[test]
fn delivery_migration_preserves_existing_dispatch_without_inventing_received_input() {
	let home = tempfile::tempdir().unwrap();
	let mut c = Connection::open(home.path().join("upgrade.db")).unwrap();
	migrations::configure(&c).unwrap();
	for m in MIGRATIONS.iter().filter(|m| m.version <= 56) {
		c.execute_batch(m.sql).unwrap();
		c.execute(
			"INSERT INTO schema_migrations VALUES(?1,?2,?3,1)",
			params![m.version, m.name, migrations::migration_digest(m.sql)],
		)
		.unwrap();
	}
	c.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
	c.pragma_update(None, "user_version", 56).unwrap();
	c.execute_batch("BEGIN; INSERT INTO agent_work_items(id,kind,title,instructions,codex_thread_id,status,created_at_micros,updated_at_micros) VALUES('sage','goal','Sage','Plan','native-sage','open',1,1); INSERT INTO agents VALUES('sage',0); INSERT INTO works(id,owner_agent_id,title,requirements_id,state) VALUES('venue','sage','Venue','r1','open'); INSERT INTO work_requirements VALUES('venue','r1','Plan','Verify','User'); INSERT INTO native_work_contributions(id,work_id,requirements_id,instruction) VALUES('child','venue','r1','Inspect the venue'); INSERT INTO native_work_dispatches VALUES('child','native-sage','parent-turn','native-call'); COMMIT;").unwrap();
	migrations::migrate(&mut c).unwrap();
	migrations::verify(&c).unwrap();
	let retained = c
		.query_row("SELECT thread_id,turn_id,call_id FROM native_work_dispatches", [], |r| {
			Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
		})
		.unwrap();
	assert_eq!(retained, ("native-sage".into(), "parent-turn".into(), "native-call".into()));
	assert_eq!(
		c.query_row("SELECT count(*) FROM native_work_deliveries", [], |r| r.get::<_, i64>(0))
			.unwrap(),
		0
	);
	assert!(
		c.execute(
			"INSERT INTO native_work_deliveries(contribution_id,child_turn_id) VALUES('child','partial')",
			[]
		)
		.is_err(),
		"a receiving turn needs its exact message receipt"
	);
}
