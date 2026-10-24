//! Directory scopes independent of agent ownership.
use crate::{SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{Connection, OptionalExtension as _};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
	pub id: String,
	pub name: String,
	pub directory: String,
	pub agent_ids: Vec<String>,
	pub native_project_id: Option<String>,
	pub lead_agent_id: Option<String>,
}
pub(crate) fn read_projects(connection: &Connection) -> Result<Vec<Project>, StoreError> {
	let mut statement=connection.prepare("SELECT w.id,w.name,w.directory,m.agent_id,w.native_project_id,w.lead_agent_id FROM projects w LEFT JOIN project_agents m ON m.project_id=w.id ORDER BY w.name,w.id,m.agent_id").map_err(sqlite_error)?;
	let mut rows = statement.query([]).map_err(sqlite_error)?;
	let mut result: Vec<Project> = Vec::new();
	while let Some(row) = rows.next().map_err(sqlite_error)? {
		let id: String = row.get(0).map_err(sqlite_error)?;
		if result.last().is_none_or(|w| w.id != id) {
			result.push(Project {
				id,
				name: row.get(1).map_err(sqlite_error)?,
				directory: row.get(2).map_err(sqlite_error)?,
				agent_ids: Vec::new(),
				native_project_id: row.get(4).map_err(sqlite_error)?,
				lead_agent_id: row.get(5).map_err(sqlite_error)?,
			});
		}
		if let Some(work) = row.get::<_, Option<String>>(3).map_err(sqlite_error)? {
			result
				.last_mut()
				.expect("current workspace was inserted before its members")
				.agent_ids
				.push(work);
		}
	}

	Ok(result)
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
			|| directory.len() > 4096
		{
			return Err(StoreError::InvalidInput("invalid workspace"));
		}
		self.run(move |c| {
			c.execute(
				"INSERT INTO projects(id,name,directory) VALUES(?1,?2,?3) ON CONFLICT(directory) DO NOTHING",
				rusqlite::params![id, name, directory],
			)
			.map_err(sqlite_error)?;
			c.query_row("SELECT id FROM projects WHERE directory=?1", [directory], |r| r.get(0))
				.map_err(|e| sqlite_error(e).into())
		})
		.await
	}

	pub async fn work_directory(&self, id: String) -> Result<Option<String>, StoreError> {
		self.run(move |c| {
			c.query_row(
				"SELECT directory FROM projects JOIN project_agents ON project_id=id WHERE agent_id=?1",
				[id],
				|r| r.get(0),
			)
			.optional()
			.map_err(|e| sqlite_error(e).into())
		})
		.await
	}
}

impl SqliteStore {
	pub async fn bind_native_project(
		&self,
		id: String,
		native: String,
		name: String,
	) -> Result<(), StoreError> {
		self.run(move|c|{let n=c.execute("UPDATE projects SET native_project_id=?2,name=?3 WHERE id=?1 AND (native_project_id IS NULL OR native_project_id=?2)",rusqlite::params![id,native,name]).map_err(sqlite_error)?;if n!=1{return Err(crate::DatabaseError::Conflict.into());}Ok(())}).await
	}

	pub async fn set_project_lead(&self, id: String, agent: String) -> Result<(), StoreError> {
		self.run(move|c|{let n=c.execute("UPDATE projects SET lead_agent_id=?2 WHERE id=?1 AND EXISTS(SELECT 1 FROM agents WHERE id=?2)",rusqlite::params![id,agent]).map_err(sqlite_error)?;if n!=1{return Err(crate::DatabaseError::Conflict.into());}Ok(())}).await
	}
}
