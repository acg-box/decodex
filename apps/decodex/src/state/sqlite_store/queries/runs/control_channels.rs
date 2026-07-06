use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, PathBuf, Result, RunControlChannelRecord, StateData},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_run_control_channels(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, transport, channel_path, status, \
			 published_at, published_at_unix, updated_at, updated_at_unix \
			 FROM run_control_channels",
		)?;
		let rows = statement.query_map([], |row| {
			Ok(RunControlChannelRecord {
				run_id: row.get(0)?,
				project_id: row.get(1)?,
				issue_id: row.get(2)?,
				attempt_number: row.get(3)?,
				transport: row.get(4)?,
				channel_path: PathBuf::from(row.get::<_, String>(5)?),
				status: row.get(6)?,
				published_at: row.get(7)?,
				published_at_unix: row.get(8)?,
				updated_at: row.get(9)?,
				updated_at_unix: row.get(10)?,
			})
		})?;

		for row in rows {
			let channel = row?;

			state.control_channels.insert(channel.run_id.clone(), channel);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_control_channels_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, transport, channel_path, status, \
			 published_at, published_at_unix, updated_at, updated_at_unix \
			 FROM run_control_channels WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(queries::params![project_id], |row| {
			Ok(RunControlChannelRecord {
				run_id: row.get(0)?,
				project_id: row.get(1)?,
				issue_id: row.get(2)?,
				attempt_number: row.get(3)?,
				transport: row.get(4)?,
				channel_path: PathBuf::from(row.get::<_, String>(5)?),
				status: row.get(6)?,
				published_at: row.get(7)?,
				published_at_unix: row.get(8)?,
				updated_at: row.get(9)?,
				updated_at_unix: row.get(10)?,
			})
		})?;

		for row in rows {
			let channel = row?;

			state.control_channels.insert(channel.run_id.clone(), channel);
		}

		Ok(())
	}
}
