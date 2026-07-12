use crate::{
	prelude::Result,
	state::{
		StateData,
		sqlite_store::{OptionalExtension, SqliteStateStore},
	},
};

impl SqliteStateStore {
	pub(super) fn load_tracker_workspace_directory(&self, state: &mut StateData) -> Result<()> {
		let payload = self
			.connection
			.query_row(
				"SELECT payload_json FROM tracker_workspace_directory WHERE singleton = 1",
				[],
				|row| row.get::<_, String>(0),
			)
			.optional()?;
		if let Some(payload) = payload {
			state.tracker_workspace_directory = serde_json::from_str(&payload)?;
		}

		Ok(())
	}
}
