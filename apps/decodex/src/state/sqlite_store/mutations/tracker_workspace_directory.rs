use crate::{
	prelude::Result,
	state::sqlite_store::{SqliteStateStore, params},
	tracker::TrackerWorkspaceDirectory,
};

impl SqliteStateStore {
	pub(in crate::state) fn persist_tracker_workspace_directory(
		&self,
		directory: &TrackerWorkspaceDirectory,
	) -> Result<()> {
		let payload = serde_json::to_string(directory)?;
		self.connection.execute(
			"INSERT INTO tracker_workspace_directory (singleton, payload_json) VALUES (1, ?1)
			 ON CONFLICT(singleton) DO UPDATE SET payload_json = excluded.payload_json",
			params![payload],
		)?;

		Ok(())
	}
}
