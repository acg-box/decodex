//! Direct native context replacement. Creation and retirement receipts remain separate.
use rusqlite::{OptionalExtension as _, TransactionBehavior};

use crate::{DatabaseError, SqliteStore, StoreError, error};

impl SqliteStore {
	pub async fn reserve_agent_tool_migration(
		&self,
		agent: String,
		thread: String,
		parameters: String,
	) -> Result<bool, StoreError> {
		self.run(move|c|{
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(crate::error::sqlite_error)?;
			let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_tool_migrations WHERE agent_id=?1)",[&agent],|r|r.get(0)).map_err(crate::error::sqlite_error)?;

			if pending{return Ok(false);}

			let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agents a JOIN agent_work_items w ON w.id=a.id LEFT JOIN agent_tool_versions v ON v.work_id=a.id WHERE a.id=?1 AND w.codex_thread_id=?2 AND w.dispatch_state='idle' AND w.active_turn_id IS NULL AND coalesce(v.version,1)<4)",rusqlite::params![agent,thread],|r|r.get(0)).map_err(crate::error::sqlite_error)?;

			if !valid{return Err(DatabaseError::Conflict.into());}

			tx.execute("INSERT INTO agent_tool_migrations(agent_id,old_thread_id,parameters_json,outcome) VALUES(?1,?2,?3,'reserved')",rusqlite::params![agent,thread,parameters]).map_err(crate::error::sqlite_error)?;

			tx.commit().map_err(crate::error::sqlite_error)?;Ok(true)
		}).await
	}

	pub async fn finish_agent_tool_migration(
		&self,
		agent: String,
		old_thread: String,
		new_thread: Option<String>,
	) -> Result<(), StoreError> {
		self.run(move|c|{
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(crate::error::sqlite_error)?;
			let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_tool_migrations m JOIN agent_work_items w ON w.id=m.agent_id WHERE m.agent_id=?1 AND m.old_thread_id=?2 AND m.outcome='retiring' AND w.codex_thread_id=?2 AND w.dispatch_state='idle')",rusqlite::params![agent,old_thread],|r|r.get(0)).map_err(crate::error::sqlite_error)?;

			if !valid{return Err(DatabaseError::Conflict.into());}

			if let Some(thread)=new_thread {
				let candidate:String=tx.query_row("SELECT new_thread_id FROM agent_tool_migrations WHERE agent_id=?1",[&agent],|r|r.get(0)).map_err(crate::error::sqlite_error)?;

				if candidate!=thread{return Err(DatabaseError::Conflict.into());}

				tx.execute("INSERT INTO agent_thread_revisions VALUES(?1,?2,?3,?4)",rusqlite::params![agent,old_thread,thread,crate::unix_micros()?]).map_err(crate::error::sqlite_error)?;
				tx.execute("UPDATE agent_work_items SET codex_thread_id=?2 WHERE id=?1",rusqlite::params![agent,thread]).map_err(crate::error::sqlite_error)?;
				tx.execute("INSERT INTO agent_tool_versions(work_id,version) VALUES(?1,4) ON CONFLICT(work_id) DO UPDATE SET version=4",[&agent]).map_err(crate::error::sqlite_error)?;
				tx.execute("UPDATE agent_tool_migrations SET new_thread_id=?2,outcome='applied' WHERE agent_id=?1",rusqlite::params![agent,thread]).map_err(crate::error::sqlite_error)?;
			}else{return Err(DatabaseError::Conflict.into());}

			tx.commit().map_err(crate::error::sqlite_error)?;Ok(())
		}).await
	}

	pub async fn agent_tool_migration_state(
		&self,
		agent: String,
	) -> Result<Option<String>, StoreError> {
		self.run(move |c| {
			c.query_row(
				"SELECT outcome FROM agent_tool_migrations WHERE agent_id=?1",
				[agent],
				|r| r.get(0),
			)
			.optional()
			.map_err(|e| error::sqlite_error(e).into())
		})
		.await
	}
}

impl SqliteStore {
	/// Read the saved replacement context and parameters; no missing receipt permits replay.
	pub async fn agent_tool_migration(
		&self,
		agent: String,
	) -> Result<Option<(String, Option<String>, String, String)>, StoreError> {
		self.run(move|c|c.query_row("SELECT old_thread_id,new_thread_id,parameters_json,outcome FROM agent_tool_migrations WHERE agent_id=?1",[agent],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|e|error::sqlite_error(e).into())).await
	}

	/// Save a known creation before verification or retirement. Never change a known native ID.
	pub async fn record_agent_tool_context(
		&self,
		agent: String,
		thread: Option<String>,
	) -> Result<(), StoreError> {
		self.run(move|c|{let updated=c.execute("UPDATE agent_tool_migrations SET new_thread_id=?2,outcome=CASE WHEN ?2 IS NULL THEN 'unknown' ELSE 'created' END WHERE agent_id=?1 AND outcome='reserved'",rusqlite::params![agent,thread]).map_err(crate::error::sqlite_error)?;if updated!=1{return Err(DatabaseError::Conflict.into());}Ok(())}).await
	}

	/// Record archive intent before stopping the exact old native context.
	pub async fn retire_agent_tool_context(&self, agent: String) -> Result<(), StoreError> {
		self.run(move|c|{let updated=c.execute("UPDATE agent_tool_migrations SET outcome='retiring' WHERE agent_id=?1 AND outcome IN('created','retiring')",[agent]).map_err(crate::error::sqlite_error)?;if updated!=1{return Err(DatabaseError::Conflict.into());}Ok(())}).await
	}
}
