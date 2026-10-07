//! Directory scopes independent of agent ownership.
use crate::{SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{Connection, OptionalExtension as _};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
	pub id: String,
	pub name: String,
	pub directory: String,
	pub work_ids: Vec<String>,
}
pub(crate) fn read_workspaces(connection: &Connection) -> Result<Vec<Workspace>, StoreError> {
	let mut statement=connection.prepare("SELECT w.id,w.name,w.directory,m.work_id FROM workspaces w LEFT JOIN work_workspace m ON m.workspace_id=w.id ORDER BY w.name,w.id,m.work_id").map_err(sqlite_error)?;
	let mut rows = statement.query([]).map_err(sqlite_error)?;
	let mut result: Vec<Workspace> = Vec::new();
	while let Some(row) = rows.next().map_err(sqlite_error)? {
		let id: String = row.get(0).map_err(sqlite_error)?;
		if result.last().is_none_or(|w| w.id != id) {
			result.push(Workspace {
				id,
				name: row.get(1).map_err(sqlite_error)?,
				directory: row.get(2).map_err(sqlite_error)?,
				work_ids: Vec::new(),
			});
		}
		if let Some(work) = row.get::<_, Option<String>>(3).map_err(sqlite_error)? {
			result
				.last_mut()
				.expect("current workspace was inserted before its members")
				.work_ids
				.push(work);
		}
	}

	Ok(result)
}
impl SqliteStore {
	pub async fn workspaces(&self) -> Result<Vec<Workspace>, StoreError> {
		self.run(|c| read_workspaces(c)).await
	}

	/// Register an existing canonical directory without creating any work or message.
	pub async fn register_workspace(
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
				"INSERT INTO workspaces(id,name,directory) VALUES(?1,?2,?3) ON CONFLICT(directory) DO NOTHING",
				rusqlite::params![id, name, directory],
			)
			.map_err(sqlite_error)?;
			c.query_row("SELECT id FROM workspaces WHERE directory=?1", [directory], |r| r.get(0))
				.map_err(|e| sqlite_error(e).into())
		})
		.await
	}

	pub async fn work_directory(&self, id: String) -> Result<Option<String>, StoreError> {
		self.run(move |c| {
			c.query_row(
				"SELECT directory FROM workspaces JOIN work_workspace ON workspace_id=id WHERE work_id=?1",
				[id],
				|r| r.get(0),
			)
			.optional()
			.map_err(|e| sqlite_error(e).into())
		})
		.await
	}
}
