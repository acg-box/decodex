//! Confirm independent conversations on an already admitted native process.
use super::{SqliteStore, StoreError, account_lifecycle, owns_work};
use rusqlite::{OptionalExtension as _, TransactionBehavior};
use sha2::{Digest as _, Sha256};

impl SqliteStore {
	/// Call only after this exact process created or resumed the native thread.
	/// Membership in a Project or the Agent role alone never grants process ownership.
	pub async fn attach_independent_agent_process(
		&self,
		work: String,
		thread: String,
		generation: String,
	) -> Result<bool, StoreError> {
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(account_lifecycle::sql_error)?;
			let bound: bool = tx.query_row(
				"SELECT EXISTS(SELECT 1 FROM agents a JOIN agent_work_items w ON w.id=a.id WHERE w.id=?1 AND w.codex_thread_id=?2 AND w.parent_goal_id IS NULL)",
				rusqlite::params![work, thread], |r| r.get(0),
			).map_err(account_lifecycle::sql_error)?;
			if !bound { return Ok(false); }
			if owns_work(&tx, &work, Some(&generation))? { return Ok(true); }
			let account: Option<String> = tx.query_row(
				"SELECT b.account_id FROM agent_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id WHERE b.generation_id=?1 AND g.state='ready' AND b.rowid=(SELECT rowid FROM agent_process_bindings WHERE root_id=b.root_id ORDER BY created_at_micros DESC,rowid DESC LIMIT 1) LIMIT 1",
				[&generation], |r| r.get(0),
			).optional().map_err(account_lifecycle::sql_error)?;
			let Some(account) = account else { return Ok(false); };
			let conflict: bool = tx.query_row(
				"SELECT EXISTS(SELECT 1 FROM agent_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id WHERE b.root_id=?1 AND b.generation_id<>?2 AND g.state<>'dead')",
				rusqlite::params![work, generation], |r| r.get(0),
			).map_err(account_lifecycle::sql_error)?;
			if conflict { return Ok(false); }
			let digest: String = Sha256::digest(serde_json::json!([work, thread, generation]).to_string().as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
			tx.execute(
				"INSERT INTO agent_process_bindings(operation_key,root_id,account_id,generation_id,request_sha256,created_at_micros) VALUES(?1,?2,?3,?4,?5,?6)",
				rusqlite::params![format!("native-attachment:{digest}"), work, account, generation, digest, crate::unix_micros()?],
			).map_err(account_lifecycle::sql_error)?;
			tx.commit().map_err(account_lifecycle::sql_error)?;
			Ok(true)
		}).await
	}
}
