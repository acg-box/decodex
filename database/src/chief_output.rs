//! Bounded partial provider output. These records never authorize execution.
use crate::{SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{OptionalExtension as _, params};

pub struct ChiefLiveOutput {
	pub id: i64,
	pub turn_id: String,
	pub item_id: String,
	pub text: String,
	pub truncated: bool,
}

impl SqliteStore {
	pub async fn update_chief_output(
		&self,
		thread: String,
		turn: String,
		item: String,
		text: String,
		replace: bool,
	) -> Result<(), StoreError> {
		if thread.len() > 512 || turn.len() > 512 || item.len() > 512 || item.is_empty() {
			return Err(StoreError::InvalidInput("invalid live output identity"));
		}

		self.run(move |connection| {
            let work: Option<String> = connection.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'", params![thread,turn], |row|row.get(0)).optional().map_err(sqlite_error)?;
            let Some(work) = work else { return Ok(()); };
            let previous: Option<(String,bool)> = connection.query_row("SELECT text,truncated FROM chief_live_output WHERE work_id=?1 AND turn_id=?2 AND item_id=?3", params![work,turn,item], |row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(sqlite_error)?;
            if previous.is_none() {
                let count: i64 = connection.query_row("SELECT count(*) FROM chief_live_output WHERE work_id=?1 AND turn_id=?2",params![work,turn],|row|row.get(0)).map_err(sqlite_error)?;
                if count >= 32 { return Ok(()); }
            }
            let (mut content, was_truncated) = if replace { (text, false) } else { let (mut prior, truncated) = previous.unwrap_or_default(); prior.push_str(&text); (prior, truncated) };
            let truncated = was_truncated || content.len() > 65536;
            let mut end = content.len().min(65536);
            while !content.is_char_boundary(end) { end -= 1; }
            content.truncate(end);
            connection.execute("DELETE FROM chief_live_output WHERE work_id=?1 AND turn_id<>?2",params![work,turn]).map_err(sqlite_error)?;
            connection.execute("INSERT INTO chief_live_output(work_id,turn_id,item_id,text,truncated) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(work_id,turn_id,item_id) DO UPDATE SET text=excluded.text,truncated=excluded.truncated",params![work,turn,item,content,truncated]).map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	pub async fn read_chief_output(
		&self,
		work: String,
	) -> Result<Vec<ChiefLiveOutput>, StoreError> {
		self.run(move |connection| read_live(connection, &work)).await
	}
}

impl SqliteStore {
	pub async fn chief_tool_version(&self, id: String) -> Result<i64, StoreError> {
		self.run(move |connection| {
			Ok(connection
				.query_row(
					"SELECT version FROM chief_tool_versions WHERE work_id=?1",
					[id],
					|row| row.get(0),
				)
				.optional()
				.map_err(sqlite_error)?
				.unwrap_or(1))
		})
		.await
	}

	pub async fn begin_chief_tool_upgrade(
		&self,
		id: String,
		old_thread: String,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
            let changed=connection.execute("UPDATE chief_work_items SET dispatch_state='dispatching' WHERE id=?1 AND codex_thread_id=?2 AND dispatch_state='idle' AND active_turn_id IS NULL",params![id,old_thread]).map_err(sqlite_error)?;
            if changed!=1 { return Err(crate::DatabaseError::Conflict.into()); }
            Ok(())
        }).await
	}

	pub async fn finish_chief_tool_upgrade(
		&self,
		id: String,
		old_thread: String,
		new_thread: String,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let changed=tx.execute("UPDATE chief_work_items SET codex_thread_id=?3,dispatch_state='idle' WHERE id=?1 AND codex_thread_id=?2 AND dispatch_state='dispatching' AND active_turn_id IS NULL",params![id,old_thread,new_thread]).map_err(sqlite_error)?;
            if changed!=1 { return Err(crate::DatabaseError::Conflict.into()); }
            tx.execute("INSERT INTO chief_thread_revisions(work_id,old_thread_id,new_thread_id,created_at_micros) VALUES(?1,?2,?3,?4)",params![id,old_thread,new_thread,crate::unix_micros()?]).map_err(sqlite_error)?;
            tx.execute("INSERT INTO chief_tool_versions(work_id,version) VALUES(?1,2) ON CONFLICT(work_id) DO UPDATE SET version=2",[id]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}
}

pub(crate) fn read_live(
	connection: &rusqlite::Connection,
	work: &str,
) -> Result<Vec<ChiefLiveOutput>, StoreError> {
	connection.prepare("SELECT o.id,o.turn_id,o.item_id,o.text,o.truncated FROM chief_live_output o JOIN chief_work_items w ON w.id=o.work_id AND w.active_turn_id=o.turn_id WHERE w.id=?1 AND w.dispatch_state IN ('running','unknown') ORDER BY o.id LIMIT 32").map_err(sqlite_error)?
        .query_map([work],|row|Ok(ChiefLiveOutput {id:row.get(0)?,turn_id:row.get(1)?,item_id:row.get(2)?,text:row.get(3)?,truncated:row.get(4)?})).map_err(sqlite_error)?
        .collect::<Result<Vec<_>,_>>().map_err(|error|sqlite_error(error).into())
}

impl SqliteStore {
	/// Save bounded usage only for a known provider thread and active turn.
	pub async fn update_chief_usage(
		&self,
		thread: String,
		turn: String,
		usage: String,
	) -> Result<(), StoreError> {
		if usage.len() > 1024 || thread.len() > 512 || turn.len() > 512 {
			return Err(StoreError::InvalidInput("invalid usage record"));
		}
		self.run(move |connection| {
			connection.execute("INSERT INTO chief_usage(thread_id,work_id,turn_id,usage_json) SELECT ?1,id,?2,?3 FROM chief_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 ON CONFLICT(thread_id) DO UPDATE SET
turn_input_tokens=CASE WHEN chief_usage.turn_id=excluded.turn_id AND json_extract(excluded.usage_json,'$.input_tokens')>=json_extract(chief_usage.usage_json,'$.input_tokens') THEN json_extract(excluded.usage_json,'$.input_tokens')-baseline_input_tokens END,
turn_output_tokens=CASE WHEN chief_usage.turn_id=excluded.turn_id AND json_extract(excluded.usage_json,'$.output_tokens')>=json_extract(chief_usage.usage_json,'$.output_tokens') THEN json_extract(excluded.usage_json,'$.output_tokens')-baseline_output_tokens END,
baseline_input_tokens=CASE WHEN chief_usage.turn_id=excluded.turn_id AND json_extract(excluded.usage_json,'$.input_tokens')>=json_extract(chief_usage.usage_json,'$.input_tokens') THEN baseline_input_tokens END,
baseline_output_tokens=CASE WHEN chief_usage.turn_id=excluded.turn_id AND json_extract(excluded.usage_json,'$.output_tokens')>=json_extract(chief_usage.usage_json,'$.output_tokens') THEN baseline_output_tokens END,
turn_id=excluded.turn_id,usage_json=excluded.usage_json", params![thread,turn,usage]).map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Read only the selected work's current provider-thread usage.
	pub async fn read_chief_usage(&self, work: String) -> Result<Option<String>, StoreError> {
		self.run(move |connection| {
			connection.query_row("SELECT u.usage_json FROM chief_usage u JOIN chief_work_items w ON u.work_id=w.id AND u.thread_id=w.codex_thread_id WHERE w.id=?1", [work], |row| row.get(0)).optional().map_err(|error| sqlite_error(error).into())
		}).await
	}
}

impl SqliteStore {
	/// A newly created provider thread has no consumed tokens. Context remains unobserved.
	pub async fn initialize_chief_usage(&self, thread: String) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection.execute("INSERT OR IGNORE INTO chief_usage(thread_id,work_id,turn_id,usage_json) SELECT ?1,id,'','{\"input_tokens\":0,\"output_tokens\":0}' FROM chief_work_items WHERE codex_thread_id=?1",[thread]).map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// An external turn invalidates the old counter baseline and context snapshot.
	pub async fn validate_chief_usage_resume(
		&self,
		thread: String,
		last_turn: Option<String>,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection
				.execute(
					"DELETE FROM chief_usage WHERE thread_id=?1 AND (?2 IS NULL OR turn_id<>?2)",
					params![thread, last_turn],
				)
				.map_err(sqlite_error)?;
			Ok(())
		})
		.await
	}

	/// Return a complete turn delta only when its baseline and current counters are known.
	pub async fn read_chief_turn_usage(
		&self,
		thread: String,
		turn: String,
	) -> Result<Option<(u64, u64)>, StoreError> {
		self.run(move |connection| {
			connection.query_row("SELECT turn_input_tokens,turn_output_tokens FROM chief_usage WHERE thread_id=?1 AND turn_id=?2 AND turn_input_tokens IS NOT NULL AND turn_output_tokens IS NOT NULL",params![thread,turn],|row|Ok((row.get::<_,i64>(0)? as u64,row.get::<_,i64>(1)? as u64))).optional().map_err(|error|sqlite_error(error).into())
		}).await
	}
}

impl SqliteStore {
	/// Restore an explicitly expected native replay without replacing newer live usage.
	pub async fn restore_chief_usage(
		&self,
		thread: String,
		turn: String,
		usage: String,
	) -> Result<(), StoreError> {
		if usage.len() > 1024 || thread.len() > 512 || turn.len() > 512 {
			return Err(StoreError::InvalidInput("invalid usage replay"));
		}
		self.run(move |connection| {
			connection.execute("INSERT INTO chief_usage(thread_id,work_id,turn_id,usage_json,baseline_input_tokens,baseline_output_tokens)
SELECT ?1,id,coalesce(active_turn_id,?2),?3,json_extract(?3,'$.input_tokens'),json_extract(?3,'$.output_tokens') FROM chief_work_items WHERE codex_thread_id=?1 AND (active_turn_id IS NULL OR active_turn_id<>?2)
ON CONFLICT(thread_id) DO UPDATE SET turn_id=excluded.turn_id,usage_json=excluded.usage_json,baseline_input_tokens=excluded.baseline_input_tokens,baseline_output_tokens=excluded.baseline_output_tokens,turn_input_tokens=NULL,turn_output_tokens=NULL
WHERE (chief_usage.turn_input_tokens IS NULL AND chief_usage.turn_output_tokens IS NULL) OR EXISTS(SELECT 1 FROM chief_work_items WHERE id=excluded.work_id AND active_turn_id IS NULL)",params![thread,turn,usage]).map_err(sqlite_error)?;
			Ok(())
		}).await
	}
}

impl SqliteStore {
	/// Append an immutable activity receipt without changing work status or waking an agent.
	pub async fn record_chief_activity(
		&self,
		thread: String,
		turn: String,
		item: String,
		completed: bool,
		payload: String,
	) -> Result<(), StoreError> {
		if thread.len() > 512
			|| turn.len() > 512
			|| item.is_empty()
			|| item.len() > 512
			|| payload.len() > 4096
		{
			return Err(StoreError::InvalidInput("invalid activity receipt"));
		}
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work:Option<String>=tx.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'",params![thread,turn],|row|row.get(0)).optional().map_err(sqlite_error)?;
			let Some(work)=work else { return Ok(()); };
			let count:i64=tx.query_row("SELECT count(*) FROM chief_inbox_events WHERE work_item_id=?1 AND delivered_turn_id=?2 AND event_kind IN ('activity_started','activity_completed')",params![work,turn],|row|row.get(0)).map_err(sqlite_error)?;
			if count>=256 { return Ok(()); }
			let stage=if completed {"completed"} else {"started"};
			let source=serde_json::json!(["activity",work,turn,item,stage]).to_string();
			let now=crate::unix_micros()?;
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,?3,?4,?5,'resolved','Observed execution activity',?5,?2,?6) ON CONFLICT(source_event_id) DO NOTHING",params![source,work,format!("activity_{stage}"),payload,now,turn]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}
}
