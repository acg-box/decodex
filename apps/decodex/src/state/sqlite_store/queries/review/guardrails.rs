use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{LoopGuardrailKey, LoopGuardrailRuntimeRecord, Result, StateData},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_loop_guardrail_checkpoints(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, reason, fingerprint, run_id, attempt_number, \
			 consecutive_count, details_json, updated_at, updated_at_unix \
			 FROM loop_guardrail_checkpoints",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let reason: String = row.get(2)?;

			Ok((
				LoopGuardrailKey::new(&project_id, &issue_id, &reason),
				LoopGuardrailRuntimeRecord {
					project_id,
					issue_id,
					reason,
					fingerprint: row.get(3)?,
					run_id: row.get(4)?,
					attempt_number: row.get(5)?,
					consecutive_count: row.get(6)?,
					details_json: row.get(7)?,
					updated_at: row.get(8)?,
					updated_at_unix: row.get(9)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.loop_guardrail_checkpoints.insert(key, record);
		}

		Ok(())
	}
}
