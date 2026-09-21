//! Bounded partial provider output. These records never authorize execution.
use crate::{SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{OptionalExtension as _, params};

pub struct ChiefOutputUpdate {
	pub thread_id: String,
	pub turn_id: String,
	pub item_id: String,
	pub kind: String,
	pub text: String,
	pub completed: bool,
}

pub struct ChiefLiveOutput {
	pub id: i64,
	pub turn_id: String,
	pub item_id: String,
	pub text: String,
	pub truncated: bool,
	pub kind: String,
}

impl SqliteStore {
	/// Save a configuration warning only for the current ready process of this root.
	pub async fn record_chief_config_warning(
		&self,
		root: String,
		generation: String,
		digest: String,
		text: String,
	) -> Result<(), StoreError> {
		if root.is_empty()
			|| root.len() > 512
			|| generation.len() > 128
			|| digest.len() != 64
			|| text.len() > 32768
		{
			return Err(StoreError::InvalidInput("invalid configuration warning"));
		}
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let owned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id WHERE b.root_id=?1 AND b.generation_id=?2 AND g.state='ready' AND b.rowid=(SELECT rowid FROM chief_process_bindings WHERE root_id=?1 ORDER BY created_at_micros DESC,rowid DESC LIMIT 1))",params![root,generation],|r|r.get(0)).map_err(sqlite_error)?;
			if !owned { return Ok(()); }
			let original_source=serde_json::json!(["config_warning",root,generation,digest]).to_string();
			if tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE source_event_id=?1)",[original_source],|r|r.get::<_,bool>(0)).map_err(sqlite_error)? { return Ok(()); }
			let count: i64 = tx.query_row("SELECT count(*) FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind='config_warning' AND json_extract(payload,'$.generation')=?2",params![root,generation],|r|r.get(0)).map_err(sqlite_error)?;
			let (digest,text) = if count >= 64 { ("overflow".to_owned(),"Additional configuration warnings exceeded the display limit.".to_owned()) } else { (digest,text) };
			let source=serde_json::json!(["config_warning",root,generation,digest]).to_string();
			let payload=serde_json::json!({"generation":generation,"text":text}).to_string();
			let now=crate::unix_micros()?;
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'config_warning',?3,?4,'resolved','Observed native configuration warning',?4) ON CONFLICT(source_event_id) DO NOTHING",params![source,root,payload,now]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	pub async fn update_chief_output(
		&self,
		thread: String,
		turn: String,
		item: String,
		text: String,
		replace: bool,
	) -> Result<(), StoreError> {
		self.update_chief_output_record(ChiefOutputUpdate {
			thread_id: thread,
			turn_id: turn,
			item_id: item,
			kind: "agentMessage".into(),
			text,
			completed: replace,
		})
		.await
	}

	pub async fn update_chief_output_record(
		&self,
		update: ChiefOutputUpdate,
	) -> Result<(), StoreError> {
		let ChiefOutputUpdate {
			thread_id: thread,
			turn_id: turn,
			item_id: item,
			kind,
			text,
			completed: replace,
		} = update;
		if !matches!(kind.as_str(), "agentMessage" | "plan") {
			return Err(StoreError::InvalidInput("invalid live output kind"));
		}
		if thread.len() > 512 || turn.len() > 512 || item.len() > 512 || item.is_empty() {
			return Err(StoreError::InvalidInput("invalid live output identity"));
		}

		self.run(move |connection| {
            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let connection = &tx;
            let work: Option<String> = connection.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'", params![thread,turn], |row|row.get(0)).optional().map_err(sqlite_error)?;
            let Some(work) = work else { return Ok(()); };
            let previous: Option<(String,bool,String,bool)> = connection.query_row("SELECT text,truncated,kind,completed FROM chief_live_output WHERE work_id=?1 AND turn_id=?2 AND item_id=?3", params![work,turn,item], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(sqlite_error)?;
            if let Some((_,_,prior_kind,completed)) = &previous {
                if prior_kind != &kind { return Err(StoreError::InvalidInput("live output kind changed")); }
                if *completed && !replace { return Ok(()); }
            }
            if previous.is_none() {
                let count: i64 = connection.query_row("SELECT count(*) FROM chief_live_output WHERE work_id=?1 AND turn_id=?2",params![work,turn],|row|row.get(0)).map_err(sqlite_error)?;
                if count >= 32 { return Ok(()); }
            }
            let (mut content, was_truncated) = if replace { (text, false) } else { let (mut prior, truncated) = previous.map(|(text,truncated,_,_)|(text,truncated)).unwrap_or_default(); prior.push_str(&text); (prior, truncated) };
            let truncated = was_truncated || content.len() > 65536;
            let mut end = content.len().min(65536);
            while !content.is_char_boundary(end) { end -= 1; }
            content.truncate(end);
            connection.execute("DELETE FROM chief_live_output WHERE work_id=?1 AND turn_id<>?2",params![work,turn]).map_err(sqlite_error)?;
            connection.execute("INSERT INTO chief_live_output(work_id,turn_id,item_id,text,truncated,kind,completed) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(work_id,turn_id,item_id) DO UPDATE SET text=excluded.text,truncated=excluded.truncated,completed=excluded.completed",params![work,turn,item,content,truncated,kind,replace]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
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
	/// Return prior native thread identities for this exact durable work item.
	pub async fn chief_previous_threads(&self, id: String) -> Result<Vec<String>, StoreError> {
		self.run(move |connection| {
			let mut statement = connection.prepare(
				"SELECT old_thread_id FROM chief_thread_revisions WHERE work_id=?1 ORDER BY created_at_micros DESC LIMIT 129"
			).map_err(sqlite_error)?;
			let rows = statement.query_map([id], |row| row.get(0)).map_err(sqlite_error)?;
			let threads: Vec<String> = rows.collect::<Result<_,_>>().map_err(sqlite_error)?;
			if threads.len() > 128 { return Err(crate::DatabaseError::Conflict.into()); }
			Ok(threads)
		}).await
	}

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
            tx.execute("INSERT INTO chief_tool_versions(work_id,version) VALUES(?1,3) ON CONFLICT(work_id) DO UPDATE SET version=3",[id]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}
}

pub(crate) fn read_live(
	connection: &rusqlite::Connection,
	work: &str,
) -> Result<Vec<ChiefLiveOutput>, StoreError> {
	connection.prepare("SELECT o.id,o.turn_id,o.item_id,o.text,o.truncated,o.kind FROM chief_live_output o JOIN chief_work_items w ON w.id=o.work_id AND w.active_turn_id=o.turn_id WHERE w.id=?1 AND w.dispatch_state IN ('running','unknown') ORDER BY o.id LIMIT 32").map_err(sqlite_error)?
        .query_map([work],|row|Ok(ChiefLiveOutput {id:row.get(0)?,turn_id:row.get(1)?,item_id:row.get(2)?,text:row.get(3)?,truncated:row.get(4)?,kind:row.get(5)?})).map_err(sqlite_error)?
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
	/// Record a historical review notice for the exact running turn, without waking work.
	/// Later reviews in the same turn do not repeat the notice or imply a decision.
	pub async fn record_chief_strict_review(
		&self,
		thread: String,
		turn: String,
		started_at_ms: i64,
	) -> Result<(), StoreError> {
		if thread.is_empty()
			|| turn.is_empty()
			|| thread.len() > 512
			|| turn.len() > 512
			|| started_at_ms < 0
		{
			return Err(StoreError::InvalidInput("invalid strict review notice"));
		}
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work: Option<String> = tx.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'",params![thread,turn],|row|row.get(0)).optional().map_err(sqlite_error)?;
			if let Some(work) = work {
				let source = serde_json::json!(["strict_review",work,thread,turn]).to_string();
				let payload = serde_json::json!({"threadId":thread,"turnId":turn,"startedAtMs":started_at_ms}).to_string();
				let now = crate::unix_micros()?;
				tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'strict_review_notice',?3,?4,'resolved','Observed native review; no user decision requested',?4,?2,?5) ON CONFLICT(source_event_id) DO NOTHING",params![source,work,payload,now,turn]).map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

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
		// Native subagent observations can arrive after their initiating parent turn ends.
		// Only an exact saved terminal receipt can authorize that historical association.
		let historical_subagent =
			serde_json::from_str::<serde_json::Value>(&payload).is_ok_and(|value| {
				value["kind"] == "subAgentActivity"
					&& value["turn_id"] == turn
					&& value["item_id"] == item
			});
		let terminal_source = serde_json::json!(["turn/completed", thread, turn]).to_string();
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work:Option<String>=tx.query_row("SELECT w.id FROM chief_work_items w WHERE w.codex_thread_id=?1 AND ((w.active_turn_id=?2 AND w.dispatch_state='running') OR (?3 AND EXISTS(SELECT 1 FROM chief_inbox_events e WHERE e.work_item_id=w.id AND e.source_event_id=?4 AND e.event_kind IN ('chief_turn_completed','worker_turn_completed','capacity_retry'))))",params![thread,turn,historical_subagent,terminal_source],|row|row.get(0)).optional().map_err(sqlite_error)?;
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

impl SqliteStore {
	/// Save the latest observed checklist as an immutable, non-waking receipt.
	pub async fn record_chief_checklist(
		&self,
		thread: String,
		turn: String,
		text: String,
	) -> Result<(), StoreError> {
		if thread.is_empty()
			|| thread.len() > 512
			|| turn.is_empty()
			|| turn.len() > 512
			|| text.len() > 32768
		{
			return Err(StoreError::InvalidInput("invalid checklist observation"));
		}
		self.run(move |connection| {
            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let work: Option<String> = tx.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'",params![thread,turn],|row|row.get(0)).optional().map_err(sqlite_error)?;
            let Some(work) = work else { return Ok(()); };
            let previous: Option<(i64,String)> = tx.query_row("SELECT id,payload FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind='plan_updated' AND delivered_turn_id=?2 AND json_extract(payload,'$.threadId')=?3 ORDER BY id DESC LIMIT 1",params![work,turn,thread],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(sqlite_error)?;
            let original_payload = serde_json::json!({"threadId":thread,"turnId":turn,"text":text}).to_string();
            if previous.as_ref().is_some_and(|(_,prior)|prior == &original_payload) { return Ok(()); }
            let count: i64 = tx.query_row("SELECT count(*) FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind='plan_updated' AND delivered_turn_id=?2 AND json_extract(payload,'$.threadId')=?3",params![work,turn,thread],|row|row.get(0)).map_err(sqlite_error)?;
            if count > 128 { return Ok(()); }
            let text = if count == 128 { "Checklist update limit reached. Later step states are unavailable.".to_owned() } else { text };
            let payload = serde_json::json!({"threadId":thread,"turnId":turn,"text":text}).to_string();
            let source = serde_json::json!(["plan_updated",thread,turn,previous.map(|(id,_)|id)]).to_string();
            let now = crate::unix_micros()?;
            tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'plan_updated',?3,?4,'resolved','Observed native checklist',?4,?2,?5)",params![source,work,payload,now,turn]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}
}
