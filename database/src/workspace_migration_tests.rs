use crate::migrations::{self, APPLICATION_ID, MIGRATIONS};
use rusqlite::{Connection, params};

#[test]
fn workspace_migration_preserves_history_and_closest_directory_without_agent_ownership() {
	let folder = tempfile::tempdir().unwrap();
	let mut c = Connection::open(folder.path().join("upgrade.sqlite3")).unwrap();
	migrations::configure(&c).unwrap();
	for m in MIGRATIONS.iter().filter(|m| m.version < 53) {
		c.execute_batch(m.sql).unwrap();
		c.execute(
			"INSERT INTO schema_migrations VALUES(?1,?2,?3,1)",
			params![m.version, m.name, migrations::migration_digest(m.sql)],
		)
		.unwrap();
	}
	c.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
	c.pragma_update(None, "user_version", 52).unwrap();
	for (id, parent) in [
		("main", None),
		("outer", Some("main")),
		("inner", Some("outer")),
		("child", Some("inner")),
		("duplicate", Some("main")),
	] {
		c.execute("INSERT INTO agent_work_items(id,parent_goal_id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES(?1,?2,'goal',?1,'Retain instructions','open',1,1)",params![id,parent]).unwrap();
		c.execute("INSERT INTO agent_managers VALUES(?1)", [id]).unwrap();
	}
	c.execute_batch("INSERT INTO agent_workspaces VALUES('outer','Outer','/outer'),('inner','Inner','/inner'),('duplicate','Duplicate','/outer'); INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES('input','child','user_message','original input',1);").unwrap();
	migrations::migrate(&mut c).unwrap();
	let scopes:Vec<(String,String)>=c.prepare("SELECT work_id,directory FROM work_workspace JOIN workspaces ON workspace_id=id ORDER BY work_id").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?))).unwrap().collect::<Result<_,_>>().unwrap();
	assert_eq!(
		scopes,
		vec![
			("child".into(), "/inner".into()),
			("duplicate".into(), "/outer".into()),
			("inner".into(), "/inner".into()),
			("outer".into(), "/outer".into())
		]
	);
	assert_eq!(
		c.query_row("SELECT count(*) FROM workspaces", [], |r| r.get::<_, i64>(0)).unwrap(),
		2
	);
	assert_eq!(
		c.query_row(
			"SELECT payload FROM agent_inbox_events WHERE source_event_id='input'",
			[],
			|r| r.get::<_, String>(0)
		)
		.unwrap(),
		"original input"
	);
	assert_eq!(
		c.query_row("SELECT title FROM agent_work_items WHERE id='inner'", [], |r| r
			.get::<_, String>(0))
			.unwrap(),
		"Inner"
	);
	assert_eq!(
		c.query_row("SELECT count(*) FROM sqlite_schema WHERE name='agent_workspaces'", [], |r| {
			r.get::<_, i64>(0)
		})
		.unwrap(),
		0
	);
	c.execute("INSERT INTO workspaces VALUES('empty','Empty','/empty')", []).unwrap();
	assert_eq!(
		c.query_row("SELECT count(*) FROM agent_work_items", [], |r| r.get::<_, i64>(0)).unwrap(),
		5
	);
	migrations::migrate(&mut c).unwrap();
	assert_eq!(
		c.query_row("SELECT count(*) FROM workspaces", [], |r| r.get::<_, i64>(0)).unwrap(),
		3
	);
}
