use crate::state::sqlite_store::mutations::{
	self, Result, RunControlChannelRecord, SqliteStateStore,
};

impl SqliteStateStore {
	pub(in crate::state) fn upsert_run_control_channel(
		&self,
		channel: &RunControlChannelRecord,
	) -> Result<()> {
		self.connection.execute(
			"INSERT OR REPLACE INTO run_control_channels (
					run_id, project_id, issue_id, attempt_number, transport, channel_path, status,
					published_at, published_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			mutations::params![
				&channel.run_id,
				&channel.project_id,
				&channel.issue_id,
				channel.attempt_number,
				&channel.transport,
				channel.channel_path.to_string_lossy().as_ref(),
				&channel.status,
				&channel.published_at,
				channel.published_at_unix,
				&channel.updated_at,
				channel.updated_at_unix,
			],
		)?;

		Ok(())
	}
}
