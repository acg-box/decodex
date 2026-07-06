use crate::state::sqlite_store::mutations::{
	self, ChildAgentActivitySummary, Result, RunActivitySummaryRecord, SqliteStateStore,
};

impl SqliteStateStore {
	pub(in crate::state) fn upsert_run_activity_summary(
		&self,
		summary: &RunActivitySummaryRecord,
	) -> Result<()> {
		let child_agent_activity_json = summary
			.child_agent_activity
			.as_ref()
			.cloned()
			.map(ChildAgentActivitySummary::sealed_durable)
			.map(|summary| serde_json::to_string(&summary))
			.transpose()?;
		let protocol_activity_json =
			summary.protocol_activity.as_ref().map(serde_json::to_string).transpose()?;

		self.connection.execute(
			"INSERT OR REPLACE INTO run_activity_summaries (
					run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			mutations::params![
				&summary.run_id,
				summary.attempt_number,
				child_agent_activity_json.as_deref(),
				protocol_activity_json.as_deref(),
				&summary.updated_at,
				summary.updated_at_unix,
			],
		)?;

		Ok(())
	}
}
