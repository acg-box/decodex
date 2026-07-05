use crate::state::sqlite_store::schema::{
	self, ChildAgentActivitySummary, Result, SqliteStateStore,
};

impl SqliteStateStore {
	pub(in crate::state) fn seal_run_activity_summary_records(&self) -> Result<()> {
		let updates = {
			let mut statement = self.connection.prepare(
				"SELECT run_id, child_agent_activity_json FROM run_activity_summaries \
				 WHERE child_agent_activity_json IS NOT NULL",
			)?;
			let rows = statement
				.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
			let mut updates = Vec::new();

			for row in rows {
				let (run_id, child_agent_activity_json) = row?;
				let sealed_json = serde_json::to_string(
					&serde_json::from_str::<ChildAgentActivitySummary>(&child_agent_activity_json)?
						.sealed_durable(),
				)?;

				if sealed_json != child_agent_activity_json {
					updates.push((run_id, sealed_json));
				}
			}

			updates
		};

		for (run_id, child_agent_activity_json) in updates {
			self.connection.execute(
				"UPDATE run_activity_summaries SET child_agent_activity_json = ?2 WHERE run_id = ?1",
				schema::params![run_id, child_agent_activity_json],
			)?;
		}

		Ok(())
	}
}
