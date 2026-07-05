use crate::state::{
	runtime_row_parsers,
	sqlite_store::schema::{self, Result, SqliteStateStore, eyre},
};

impl SqliteStateStore {
	pub(in crate::state) fn migrate_removed_decision_contract_fields(&self) -> Result<()> {
		let updates = {
			let mut statement = self.connection.prepare(
				"SELECT project_id, contract_id, payload_json
				 FROM decision_contracts
				 WHERE json_type(payload_json, '$.execution_readiness.proposed_issue_summaries') IS NOT NULL
				 OR json_type(payload_json, '$.execution_readiness.queue_intent') IS NOT NULL
				 ORDER BY project_id ASC, contract_id ASC",
			)?;
			let rows = statement.query_map([], |row| {
				Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
			})?;
			let mut updates = Vec::new();

			for row in rows {
				let (project_id, contract_id, payload_json) = row?;
				let migrated_payload =
					runtime_row_parsers::migrate_removed_decision_contract_fields(&payload_json)
						.map_err(|error| {
							eyre::eyre!(
								"Decision Contract `{project_id}/{contract_id}` removed-field migration failed: {error}"
							)
						})?;

				if migrated_payload != payload_json {
					updates.push((project_id, contract_id, migrated_payload));
				}
			}

			updates
		};

		for (project_id, contract_id, payload_json) in updates {
			self.connection.execute(
				"UPDATE decision_contracts
				 SET payload_json = ?3
				 WHERE project_id = ?1 AND contract_id = ?2",
				schema::params![project_id, contract_id, payload_json],
			)?;
		}

		Ok(())
	}
}
