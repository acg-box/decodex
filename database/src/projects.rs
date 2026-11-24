//! Directory scopes independent of agent ownership.
use rusqlite::{Connection, OptionalExtension as _};

use crate::{DatabaseError, SqliteStore, StoreError, error};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
	pub id: String,
	pub name: String,
	pub directory: String,
	pub agent_ids: Vec<String>,
	pub native_project_id: Option<String>,
	pub lead_agent_id: Option<String>,
}

impl SqliteStore {
	pub async fn projects(&self) -> Result<Vec<Project>, StoreError> {
		self.run(|c| read_projects(c)).await
	}

	/// Register an existing canonical directory without creating any work or message.
	pub async fn register_project(
		&self,
		id: String,
		name: String,
		directory: String,
	) -> Result<String, StoreError> {
		if id.is_empty()
			|| id.len() > 512
			|| name.is_empty()
			|| name.len() > 256
			|| directory.is_empty()
			|| directory.len() > 4_096
		{
			return Err(StoreError::InvalidInput("invalid workspace"));
		}

		self.run(move |c| {
			c.execute(
				"INSERT INTO projects(id,name,directory) VALUES(?1,?2,?3) ON CONFLICT(directory) DO NOTHING",
				rusqlite::params![id, name, directory],
			)
			.map_err(crate::error::sqlite_error)?;

			c.query_row("SELECT id FROM projects WHERE directory=?1", [directory], |r| r.get(0))
				.map_err(|e| error::sqlite_error(e).into())
		})
		.await
	}

	/// Resolve an explicit creation folder before the project default. Bound threads use Codex.
	pub async fn work_directory(&self, id: String) -> Result<Option<String>, StoreError> {
		self.run(move |c| {
			c.query_row(
				"SELECT coalesce(
				 (SELECT json_extract(payload, '$.options.cwd') FROM agent_inbox_events
				  WHERE work_item_id=?1 AND event_kind IN ('user_message', 'voice_conversation_draft') ORDER BY id LIMIT 1),
				 (SELECT directory FROM projects JOIN project_agents ON project_id=id WHERE agent_id=?1))",
				[id],
				|r| r.get(0),
			)
			.map_err(|e| error::sqlite_error(e).into())
		})
		.await
	}
}

impl SqliteStore {
	/// Remove the exact registered project without deleting conversations or Work.
	pub async fn delete_project(
		&self,
		id: String,
		native: Option<String>,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(error::sqlite_error)?;
			let current: Option<Option<String>> = tx
				.query_row("SELECT native_project_id FROM projects WHERE id=?1", [&id], |r| {
					r.get(0)
				})
				.optional()
				.map_err(error::sqlite_error)?;
			let Some(current) = current else { return Ok(()) };
			if current != native {
				return Err(DatabaseError::Conflict.into());
			}
			tx.execute("UPDATE works SET project_id=NULL WHERE project_id=?1", [&id])
				.map_err(error::sqlite_error)?;
			tx.execute("DELETE FROM project_agents WHERE project_id=?1", [&id])
				.map_err(error::sqlite_error)?;
			tx.execute("DELETE FROM projects WHERE id=?1", [&id]).map_err(error::sqlite_error)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(())
		})
		.await
	}

	pub async fn bind_native_project(
		&self,
		id: String,
		native: String,
		name: String,
	) -> Result<(), StoreError> {
		self.run(move|c|{let n=c.execute("UPDATE projects SET native_project_id=?2,name=?3 WHERE id=?1 AND (native_project_id IS NULL OR native_project_id=?2)",rusqlite::params![id,native,name]).map_err(crate::error::sqlite_error)?;if n!=1{return Err(DatabaseError::Conflict.into());}Ok(())}).await
	}

	pub async fn set_project_lead(&self, id: String, agent: String) -> Result<(), StoreError> {
		self.run(move|c|{let n=c.execute("UPDATE projects SET lead_agent_id=?2 WHERE id=?1 AND EXISTS(SELECT 1 FROM agents WHERE id=?2)",rusqlite::params![id,agent]).map_err(crate::error::sqlite_error)?;if n!=1{return Err(DatabaseError::Conflict.into());}Ok(())}).await
	}
}

pub(crate) fn read_projects(connection: &Connection) -> Result<Vec<Project>, StoreError> {
	let mut statement=connection.prepare("SELECT w.id,w.name,w.directory,m.agent_id,w.native_project_id,w.lead_agent_id FROM projects w LEFT JOIN project_agents m ON m.project_id=w.id ORDER BY w.name,w.id,m.agent_id").map_err(crate::error::sqlite_error)?;
	let mut rows = statement.query([]).map_err(crate::error::sqlite_error)?;
	let mut result: Vec<Project> = Vec::new();

	while let Some(row) = rows.next().map_err(crate::error::sqlite_error)? {
		let id: String = row.get(0).map_err(crate::error::sqlite_error)?;

		if result.last().is_none_or(|w| w.id != id) {
			result.push(Project {
				id,
				name: row.get(1).map_err(crate::error::sqlite_error)?,
				directory: row.get(2).map_err(crate::error::sqlite_error)?,
				agent_ids: Vec::new(),
				native_project_id: row.get(4).map_err(crate::error::sqlite_error)?,
				lead_agent_id: row.get(5).map_err(crate::error::sqlite_error)?,
			});
		}

		if let Some(work) = row.get::<_, Option<String>>(3).map_err(crate::error::sqlite_error)? {
			result
				.last_mut()
				.expect("current workspace was inserted before its members")
				.agent_ids
				.push(work);
		}
	}

	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn delete_project_detaches_records_atomically_and_keeps_content() {
		let dir = tempfile::tempdir().unwrap();
		let file = dir.path().join("keep.txt");
		std::fs::write(&file, "Keep local project files").unwrap();
		let store = SqliteStore::open_test(&dir.path().join("projects.sqlite3")).unwrap();
		store
			.register_project("removed".into(), "Removed".into(), dir.path().display().to_string())
			.await
			.unwrap();
		store.register_project("kept".into(), "Kept".into(), "/kept".into()).await.unwrap();
		store
			.bind_native_project("removed".into(), "native".into(), "Removed".into())
			.await
			.unwrap();
		store.with_connection(|c| {
			c.execute_batch("BEGIN;
			 INSERT INTO agent_work_items(id,kind,title,instructions,codex_thread_id,dispatch_state,status,created_at_micros,updated_at_micros) VALUES('chat','goal','Keep title','Keep instructions','thread','idle','open',1,1);
			 INSERT INTO agents VALUES('chat',0);
			 INSERT INTO project_agents VALUES('chat','removed');
			 INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES('input','chat','user_message','{\"text\":\"Keep history\"}',1);
			 INSERT INTO works(id,project_id,owner_agent_id,title,requirements_id,state) VALUES('work','removed','chat','Keep Work','requirements','open');
			 INSERT INTO work_requirements VALUES('work','requirements','Goal','[]','User'); COMMIT;")
			 .map_err(error::sqlite_error)?;
			Ok(())
		}).unwrap();
		assert!(store.delete_project("removed".into(), Some("wrong-native".into())).await.is_err());
		assert_eq!(store.projects().await.unwrap().len(), 2);
		store.delete_project("removed".into(), Some("native".into())).await.unwrap();
		store.delete_project("removed".into(), Some("native".into())).await.unwrap();
		assert_eq!(store.projects().await.unwrap()[0].id, "kept");
		assert_eq!(store.get_agent_work_item("chat".into()).await.unwrap().title, "Keep title");
		store
			.with_connection(|c| {
				assert_eq!(
					c.query_row(
						"SELECT payload FROM agent_inbox_events WHERE source_event_id='input'",
						[],
						|r| r.get::<_, String>(0)
					)
					.unwrap(),
					"{\"text\":\"Keep history\"}"
				);
				assert!(
					c.query_row("SELECT project_id FROM works WHERE id='work'", [], |r| r
						.get::<_, Option<String>>(0))
						.unwrap()
						.is_none()
				);
				assert_eq!(
					c.query_row("SELECT count(*) FROM project_agents", [], |r| r.get::<_, i64>(0))
						.unwrap(),
					0
				);
				Ok(())
			})
			.unwrap();
		assert_eq!(std::fs::read_to_string(file).unwrap(), "Keep local project files");
	}
}
