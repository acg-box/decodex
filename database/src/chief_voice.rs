//! Durable authorization for native live voice. Signaling stays in runtime memory.
use crate::{DatabaseError, SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefVoiceCall {
	pub session_id: String,
	pub work_id: String,
	pub thread_id: String,
	pub generation_id: String,
	pub baseline_turn_id: Option<String>,
}

impl SqliteStore {
	/// Save one received voice transcript as history, never as queued agent input.
	pub async fn record_chief_voice_transcript(
		&self,
		session: String,
		sequence: u64,
		role: String,
		text: String,
		complete: bool,
	) -> Result<(), StoreError> {
		if !["user", "assistant"].contains(&role.as_str()) || text.len() > 32768 {
			return Err(StoreError::InvalidInput("invalid voice transcript"));
		}
		self.run(move |connection| {
            let work:String=connection.query_row("SELECT work_id FROM chief_voice_calls WHERE session_id=?1",[&session],|r|r.get(0)).map_err(sqlite_error)?;
            let source=serde_json::json!(["voice_transcript",session,sequence]).to_string();
            let payload=serde_json::json!({"text":text,"source":"voice","complete":complete}).to_string();
            let now=unix_micros()?;
            connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros)
                VALUES(?1,?2,?3,?4,?5,'resolved','Recorded live voice transcript.',?5)",params![source,work,format!("voice_{role}"),payload,now]).map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	/// Record authorization before a native subscription call can accept speech.
	pub async fn begin_chief_voice_call(&self, call: ChiefVoiceCall) -> Result<(), StoreError> {
		for value in [&call.session_id, &call.work_id, &call.thread_id, &call.generation_id] {
			if value.is_empty() || value.len() > 512 {
				return Err(StoreError::InvalidInput("invalid voice call identity"));
			}
		}
		if call.baseline_turn_id.as_ref().is_some_and(|v| v.is_empty() || v.len() > 512) {
			return Err(StoreError::InvalidInput("invalid voice baseline"));
		}
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let owned:bool=tx.query_row("WITH RECURSIVE family(id) AS (
                SELECT root_id FROM chief_process_bindings WHERE generation_id=?3
                UNION SELECT w.id FROM chief_work_items w JOIN family f ON w.parent_goal_id=f.id)
                SELECT EXISTS(SELECT 1 FROM chief_work_items w JOIN process_generations g ON g.generation_id=?3
                    WHERE w.id=?1 AND w.codex_thread_id=?2 AND w.dispatch_state IN ('idle','running')
                    AND w.id IN (SELECT id FROM family) AND g.state='ready')",
                params![call.work_id,call.thread_id,call.generation_id],|r|r.get(0)).map_err(sqlite_error)?;
            if !owned { return Err(DatabaseError::Conflict.into()); }
            tx.execute("INSERT INTO chief_voice_calls(session_id,work_id,thread_id,generation_id,baseline_turn_id,created_at_micros)
                VALUES(?1,?2,?3,?4,?5,?6)",params![call.session_id,call.work_id,call.thread_id,call.generation_id,call.baseline_turn_id,unix_micros()?]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	/// End the media authorization after native stop or positive process death.
	/// Already-started task ownership remains intact.
	pub async fn close_chief_voice_call(&self, session_id: String) -> Result<(), StoreError> {
		self.run(move |connection| {
			let changed = connection
				.execute(
					"UPDATE chief_voice_calls SET closed_at_micros=max(created_at_micros,?2)
                WHERE session_id=?1 AND closed_at_micros IS NULL",
					params![session_id, unix_micros()?],
				)
				.map_err(sqlite_error)?;
			if changed > 1 {
				return Err(DatabaseError::Conflict.into());
			}
			Ok(())
		})
		.await
	}

	/// Read calls that still require stop or restart reconciliation.
	pub async fn open_chief_voice_calls(&self) -> Result<Vec<ChiefVoiceCall>, StoreError> {
		self.run(|connection| {
			let mut query = connection
				.prepare(
					"SELECT session_id,work_id,thread_id,generation_id,baseline_turn_id
                FROM chief_voice_calls WHERE closed_at_micros IS NULL ORDER BY created_at_micros",
				)
				.map_err(sqlite_error)?;
			query
				.query_map([], |row| {
					Ok(ChiefVoiceCall {
						session_id: row.get(0)?,
						work_id: row.get(1)?,
						thread_id: row.get(2)?,
						generation_id: row.get(3)?,
						baseline_turn_id: row.get(4)?,
					})
				})
				.map_err(sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(sqlite_error)
				.map_err(Into::into)
		})
		.await
	}

	/// Observe a real native turn on a thread with a call authorization in this exact
	/// process generation. This does not start a turn or replay voice input.
	pub async fn observe_chief_voice_turn(
		&self,
		generation: String,
		thread: String,
		turn: String,
	) -> Result<bool, StoreError> {
		if turn.is_empty() || turn.len() > 512 {
			return Err(StoreError::InvalidInput("invalid native voice turn"));
		}
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let work:Option<String>=tx.query_row("SELECT work_id FROM chief_voice_calls
                WHERE thread_id=?1 AND generation_id=?2 ORDER BY created_at_micros DESC LIMIT 1",
                params![thread,generation],|row|row.get(0)).optional().map_err(sqlite_error)?;
            let Some(work)=work else {return Ok(false)};
            let old:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_voice_observed_turns WHERE generation_id=?1 AND thread_id=?2 AND turn_id=?3)
                OR EXISTS(SELECT 1 FROM chief_voice_calls WHERE generation_id=?1 AND thread_id=?2 AND baseline_turn_id=?3)",params![generation,thread,turn],|r|r.get(0)).map_err(sqlite_error)?;
            if old {return Ok(false)}
            tx.execute("INSERT INTO chief_voice_observed_turns VALUES(?1,?2,?3)",params![generation,thread,turn]).map_err(sqlite_error)?;
            let (state,active):(String,Option<String>)=tx.query_row("SELECT dispatch_state,active_turn_id FROM chief_work_items WHERE id=?1",[&work],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sqlite_error)?;
            if state=="running" && active.as_deref()==Some(&turn) {tx.commit().map_err(sqlite_error)?;return Ok(true)}
            if state!="idle" {return Err(DatabaseError::Conflict.into())}
            tx.execute("UPDATE chief_work_items SET dispatch_state='running',active_turn_id=?2,status='open',next_check_at_micros=NULL,
                updated_at_micros=max(updated_at_micros,?3) WHERE id=?1",params![work,turn,unix_micros()?]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(true)
        }).await
	}
}
