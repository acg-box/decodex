//! Bounded partial provider output. These records never authorize execution.
use crate::{SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{OptionalExtension as _, params};

pub struct AgentOutputUpdate {
	pub thread_id: String,
	pub turn_id: String,
	pub item_id: String,
	pub kind: String,
	pub text: String,
	pub completed: bool,
}

pub struct AgentLiveOutput {
	pub id: i64,
	pub turn_id: String,
	pub item_id: String,
	pub text: String,
	pub truncated: bool,
	pub kind: String,
}

impl SqliteStore {
	/// Retire display-only output after the native source invalidates its history.
	/// Submission receipts and durable conversation facts are not changed.
	pub async fn invalidate_agent_output(
		&self,
		thread: String,
		generation: Option<String>,
	) -> Result<(), StoreError> {
		if thread.is_empty() || thread.len() > 512 {
			return Err(StoreError::InvalidInput("invalid output source"));
		}
		let changed = self
			.run(move |connection| {
				let tx = connection
					.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
					.map_err(sqlite_error)?;
				let work = tx
					.prepare("SELECT id FROM agent_work_items WHERE codex_thread_id=?1")
					.map_err(sqlite_error)?
					.query_map([thread], |row| row.get::<_, String>(0))
					.map_err(sqlite_error)?
					.collect::<Result<Vec<_>, _>>()
					.map_err(sqlite_error)?;
				let mut changed = false;
				for id in work {
					if crate::agent_process::owns_work(&tx, &id, generation.as_deref())? {
						changed |= tx
							.execute("DELETE FROM agent_live_output WHERE work_id=?1", [&id])
							.map_err(sqlite_error)?
							> 0;
						changed |= tx
							.execute(
								"DELETE FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='partial_output'",
								[&id],
							)
							.map_err(sqlite_error)?
							> 0;
					}
				}
				tx.commit().map_err(sqlite_error)?;
				Ok(changed)
			})
			.await?;
		if changed {
			self.inner
				.agent_output_revision
				.send_modify(|revision| *revision = revision.wrapping_add(1));
		}
		Ok(())
	}

	/// Save a configuration warning only for the current ready process of this root.
	pub async fn record_agent_config_warning(
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
			let owned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id WHERE b.root_id=?1 AND b.generation_id=?2 AND g.state='ready' AND b.rowid=(SELECT rowid FROM agent_process_bindings WHERE root_id=?1 ORDER BY created_at_micros DESC,rowid DESC LIMIT 1))",params![root,generation],|r|r.get(0)).map_err(sqlite_error)?;
			if !owned { return Ok(()); }
			insert_warning(&tx, &root, &generation, &digest, &text, "config_warning")?;
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Retain a public warning for an exact thread owned by the current process.
	/// A process-wide warning belongs to the host root, never an arbitrary thread.
	pub async fn record_agent_native_warning(
		&self,
		root: String,
		generation: String,
		thread: Option<String>,
		digest: String,
		text: String,
	) -> Result<(), StoreError> {
		if root.is_empty()
			|| root.len() > 512
			|| generation.is_empty()
			|| generation.len() > 128
			|| digest.len() != 64
			|| text.len() > 32768
			|| thread.as_ref().is_some_and(|id| id.is_empty() || id.len() > 512)
		{
			return Err(StoreError::InvalidInput("invalid native warning"));
		}
		self.run(move |connection| {
			let tx = connection
				.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			if !crate::agent_process::owns_work(&tx, &root, Some(&generation))? {
				return Ok(());
			}
			let work = match thread {
				Some(thread) => tx
					.query_row(
						"SELECT id FROM agent_work_items WHERE codex_thread_id=?1",
						[thread],
						|row| row.get::<_, String>(0),
					)
					.optional()
					.map_err(sqlite_error)?,
				None => Some(root),
			};
			let Some(work) = work else {
				return Ok(());
			};
			if !crate::agent_process::owns_work(&tx, &work, Some(&generation))? {
				return Ok(());
			}
			insert_warning(&tx, &work, &generation, &digest, &text, "native_warning")?;
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		})
		.await
	}

	pub async fn update_agent_output(
		&self,
		thread: String,
		turn: String,
		item: String,
		text: String,
		replace: bool,
	) -> Result<(), StoreError> {
		self.update_agent_output_record(AgentOutputUpdate {
			thread_id: thread,
			turn_id: turn,
			item_id: item,
			kind: "agentMessage".into(),
			text,
			completed: replace,
		})
		.await
	}

	pub async fn update_agent_output_record(
		&self,
		update: AgentOutputUpdate,
	) -> Result<(), StoreError> {
		let AgentOutputUpdate {
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

		let changed = self.run(move |connection| {
            let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let connection = &tx;
            let work: Option<String> = connection.query_row("SELECT id FROM agent_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'", params![thread,turn], |row|row.get(0)).optional().map_err(sqlite_error)?;
            let Some(work) = work else { tx.commit().map_err(sqlite_error)?; return Ok(false); };
            let previous: Option<(String,bool,String,bool)> = connection.query_row("SELECT text,truncated,kind,completed FROM agent_live_output WHERE work_id=?1 AND turn_id=?2 AND item_id=?3", params![work,turn,item], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(sqlite_error)?;
            if let Some((_,_,prior_kind,completed)) = &previous {
                if prior_kind != &kind { return Err(StoreError::InvalidInput("live output kind changed")); }
                if *completed && !replace { return Ok(false); }
            }
            if previous.is_none() {
                let count: i64 = connection.query_row("SELECT count(*) FROM agent_live_output WHERE work_id=?1 AND turn_id=?2",params![work,turn],|row|row.get(0)).map_err(sqlite_error)?;
                if count >= 32 { return Ok(false); }
            }
            let (mut content, was_truncated) = if replace { (text, false) } else { let (mut prior, truncated) = previous.map(|(text,truncated,_,_)|(text,truncated)).unwrap_or_default(); prior.push_str(&text); (prior, truncated) };
            let truncated = was_truncated || content.len() > 65536;
            let mut end = content.len().min(65536);
            while !content.is_char_boundary(end) { end -= 1; }
            content.truncate(end);
            connection.execute("DELETE FROM agent_live_output WHERE work_id=?1 AND turn_id<>?2",params![work,turn]).map_err(sqlite_error)?;
            connection.execute("INSERT INTO agent_live_output(work_id,turn_id,item_id,text,truncated,kind,completed) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(work_id,turn_id,item_id) DO UPDATE SET text=excluded.text,truncated=excluded.truncated,completed=excluded.completed",params![work,turn,item,content,truncated,kind,replace]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(true)
        }).await?;
		if changed {
			self.inner
				.agent_output_revision
				.send_modify(|revision| *revision = revision.wrapping_add(1));
		}
		Ok(())
	}

	/// Wait without polling; subscribe before inspecting the revision to avoid lost wakeups.
	pub async fn wait_agent_output(
		&self,
		work: String,
		after: Option<u64>,
	) -> Result<(u64, Vec<AgentLiveOutput>), StoreError> {
		let mut changes = self.inner.agent_output_revision.subscribe();
		if after == Some(*changes.borrow_and_update()) {
			let _ =
				tokio::time::timeout(std::time::Duration::from_secs(20), changes.changed()).await;
		}
		let revision = *changes.borrow_and_update();
		let output = self.read_agent_output(work).await?;
		Ok((revision, output))
	}

	pub async fn read_agent_output(
		&self,
		work: String,
	) -> Result<Vec<AgentLiveOutput>, StoreError> {
		self.run(move |connection| read_live(connection, &work)).await
	}
}

impl SqliteStore {
	/// Return prior native thread identities for this exact durable work item.
	pub async fn agent_previous_threads(&self, id: String) -> Result<Vec<String>, StoreError> {
		self.run(move |connection| {
			let mut statement = connection.prepare(
				"SELECT old_thread_id FROM agent_thread_revisions WHERE work_id=?1 ORDER BY created_at_micros DESC LIMIT 129"
			).map_err(sqlite_error)?;
			let rows = statement.query_map([id], |row| row.get(0)).map_err(sqlite_error)?;
			let threads: Vec<String> = rows.collect::<Result<_,_>>().map_err(sqlite_error)?;
			if threads.len() > 128 { return Err(crate::DatabaseError::Conflict.into()); }
			Ok(threads)
		}).await
	}

	pub async fn agent_tool_version(&self, id: String) -> Result<i64, StoreError> {
		self.run(move |connection| {
			Ok(connection
				.query_row(
					"SELECT version FROM agent_tool_versions WHERE work_id=?1",
					[id],
					|row| row.get(0),
				)
				.optional()
				.map_err(sqlite_error)?
				.unwrap_or(1))
		})
		.await
	}

	pub async fn begin_agent_tool_upgrade(
		&self,
		id: String,
		old_thread: String,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            if crate::agent_prompt_edit::pending(&tx,&id)? || crate::agent_permissions::pending(&tx,&id)? || crate::agent_plugins::pending(&tx,&id)? || crate::agent_models::pending(&tx,&id)? {return Err(crate::DatabaseError::Conflict.into());}
            let changed=tx.execute("UPDATE agent_work_items SET dispatch_state='dispatching' WHERE id=?1 AND codex_thread_id=?2 AND dispatch_state='idle' AND active_turn_id IS NULL",params![id,old_thread]).map_err(sqlite_error)?;
            if changed!=1 { return Err(crate::DatabaseError::Conflict.into()); }
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	pub async fn finish_agent_tool_upgrade(
		&self,
		id: String,
		old_thread: String,
		new_thread: String,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let changed=tx.execute("UPDATE agent_work_items SET codex_thread_id=?3,dispatch_state='idle' WHERE id=?1 AND codex_thread_id=?2 AND dispatch_state='dispatching' AND active_turn_id IS NULL",params![id,old_thread,new_thread]).map_err(sqlite_error)?;
            if changed!=1 { return Err(crate::DatabaseError::Conflict.into()); }
            tx.execute("INSERT INTO agent_thread_revisions(work_id,old_thread_id,new_thread_id,created_at_micros) VALUES(?1,?2,?3,?4)",params![id,old_thread,new_thread,crate::unix_micros()?]).map_err(sqlite_error)?;
            tx.execute("INSERT INTO agent_tool_versions(work_id,version) VALUES(?1,3) ON CONFLICT(work_id) DO UPDATE SET version=3",[id]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}
}

pub(crate) fn read_live(
	connection: &rusqlite::Connection,
	work: &str,
) -> Result<Vec<AgentLiveOutput>, StoreError> {
	connection.prepare("SELECT o.id,o.turn_id,o.item_id,o.text,o.truncated,o.kind FROM agent_live_output o JOIN agent_work_items w ON w.id=o.work_id AND w.active_turn_id=o.turn_id WHERE w.id=?1 AND w.dispatch_state IN ('running','unknown') ORDER BY o.id LIMIT 32").map_err(sqlite_error)?
        .query_map([work],|row|Ok(AgentLiveOutput {id:row.get(0)?,turn_id:row.get(1)?,item_id:row.get(2)?,text:row.get(3)?,truncated:row.get(4)?,kind:row.get(5)?})).map_err(sqlite_error)?
        .collect::<Result<Vec<_>,_>>().map_err(|error|sqlite_error(error).into())
}

impl SqliteStore {
	/// Save bounded usage only for a known provider thread and active turn.
	pub async fn update_agent_usage(
		&self,
		thread: String,
		turn: String,
		usage: String,
	) -> Result<(), StoreError> {
		if usage.len() > 1024 || thread.len() > 512 || turn.len() > 512 {
			return Err(StoreError::InvalidInput("invalid usage record"));
		}
		self.run(move |connection| {
			connection.execute("INSERT INTO agent_usage(thread_id,work_id,turn_id,usage_json) SELECT ?1,id,?2,?3 FROM agent_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 ON CONFLICT(thread_id) DO UPDATE SET
turn_input_tokens=CASE WHEN agent_usage.turn_id=excluded.turn_id AND json_extract(excluded.usage_json,'$.input_tokens')>=json_extract(agent_usage.usage_json,'$.input_tokens') THEN json_extract(excluded.usage_json,'$.input_tokens')-baseline_input_tokens END,
turn_output_tokens=CASE WHEN agent_usage.turn_id=excluded.turn_id AND json_extract(excluded.usage_json,'$.output_tokens')>=json_extract(agent_usage.usage_json,'$.output_tokens') THEN json_extract(excluded.usage_json,'$.output_tokens')-baseline_output_tokens END,
baseline_input_tokens=CASE WHEN agent_usage.turn_id=excluded.turn_id AND json_extract(excluded.usage_json,'$.input_tokens')>=json_extract(agent_usage.usage_json,'$.input_tokens') THEN baseline_input_tokens END,
baseline_output_tokens=CASE WHEN agent_usage.turn_id=excluded.turn_id AND json_extract(excluded.usage_json,'$.output_tokens')>=json_extract(agent_usage.usage_json,'$.output_tokens') THEN baseline_output_tokens END,
turn_id=excluded.turn_id,usage_json=excluded.usage_json", params![thread,turn,usage]).map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Read only the selected work's current provider-thread usage.
	pub async fn read_agent_usage(&self, work: String) -> Result<Option<String>, StoreError> {
		self.run(move |connection| {
			connection.query_row("SELECT u.usage_json FROM agent_usage u JOIN agent_work_items w ON u.work_id=w.id AND u.thread_id=w.codex_thread_id WHERE w.id=?1", [work], |row| row.get(0)).optional().map_err(|error| sqlite_error(error).into())
		}).await
	}
}

impl SqliteStore {
	/// A newly created provider thread has no consumed tokens. Context remains unobserved.
	pub async fn initialize_agent_usage(&self, thread: String) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection.execute("INSERT OR IGNORE INTO agent_usage(thread_id,work_id,turn_id,usage_json) SELECT ?1,id,'','{\"input_tokens\":0,\"output_tokens\":0}' FROM agent_work_items WHERE codex_thread_id=?1",[thread]).map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// An external turn invalidates the old counter baseline and context snapshot.
	pub async fn validate_agent_usage_resume(
		&self,
		thread: String,
		last_turn: Option<String>,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection
				.execute(
					"DELETE FROM agent_usage WHERE thread_id=?1 AND (?2 IS NULL OR turn_id<>?2)",
					params![thread, last_turn],
				)
				.map_err(sqlite_error)?;
			Ok(())
		})
		.await
	}

	/// Return a complete turn delta only when its baseline and current counters are known.
	pub async fn read_agent_turn_usage(
		&self,
		thread: String,
		turn: String,
	) -> Result<Option<(u64, u64)>, StoreError> {
		self.run(move |connection| {
			connection.query_row("SELECT turn_input_tokens,turn_output_tokens FROM agent_usage WHERE thread_id=?1 AND turn_id=?2 AND turn_input_tokens IS NOT NULL AND turn_output_tokens IS NOT NULL",params![thread,turn],|row|Ok((row.get::<_,i64>(0)? as u64,row.get::<_,i64>(1)? as u64))).optional().map_err(|error|sqlite_error(error).into())
		}).await
	}
}

impl SqliteStore {
	/// Restore an explicitly expected native replay without replacing newer live usage.
	pub async fn restore_agent_usage(
		&self,
		thread: String,
		turn: String,
		usage: String,
	) -> Result<(), StoreError> {
		if usage.len() > 1024 || thread.len() > 512 || turn.len() > 512 {
			return Err(StoreError::InvalidInput("invalid usage replay"));
		}
		self.run(move |connection| {
			connection.execute("INSERT INTO agent_usage(thread_id,work_id,turn_id,usage_json,baseline_input_tokens,baseline_output_tokens)
SELECT ?1,id,coalesce(active_turn_id,?2),?3,json_extract(?3,'$.input_tokens'),json_extract(?3,'$.output_tokens') FROM agent_work_items WHERE codex_thread_id=?1 AND (active_turn_id IS NULL OR active_turn_id<>?2)
ON CONFLICT(thread_id) DO UPDATE SET turn_id=excluded.turn_id,usage_json=excluded.usage_json,baseline_input_tokens=excluded.baseline_input_tokens,baseline_output_tokens=excluded.baseline_output_tokens,turn_input_tokens=NULL,turn_output_tokens=NULL
WHERE (agent_usage.turn_input_tokens IS NULL AND agent_usage.turn_output_tokens IS NULL) OR EXISTS(SELECT 1 FROM agent_work_items WHERE id=excluded.work_id AND active_turn_id IS NULL)",params![thread,turn,usage]).map_err(sqlite_error)?;
			Ok(())
		}).await
	}
}

impl SqliteStore {
	/// Record a historical review notice for the exact running turn, without waking work.
	/// Later reviews in the same turn do not repeat the notice or imply a decision.
	pub async fn record_agent_strict_review(
		&self,
		thread: String,
		turn: String,
		started_at_ms: i64,
		generation: Option<String>,
	) -> Result<(), StoreError> {
		if thread.is_empty()
			|| turn.is_empty()
			|| thread.len() > 512
			|| turn.len() > 512
			|| started_at_ms < 0
			|| generation.as_deref().is_some_and(|id| id.trim().is_empty() || id.len() > 512)
		{
			return Err(StoreError::InvalidInput("invalid strict review notice"));
		}
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work: Option<String> = tx.query_row("SELECT id FROM agent_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'",params![thread,turn],|row|row.get(0)).optional().map_err(sqlite_error)?;
			if let Some(work) = work {
				if !crate::agent_process::owns_work(&tx, &work, generation.as_deref())? {
					return Ok(());
				}
				let source = serde_json::json!(["strict_review",work,thread,turn]).to_string();
				let payload = serde_json::json!({"threadId":thread,"turnId":turn,"startedAtMs":started_at_ms}).to_string();
				let now = crate::unix_micros()?;
				tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'strict_review_notice',?3,?4,'resolved','Observed native review; no user decision requested',?4,?2,?5) ON CONFLICT(source_event_id) DO NOTHING",params![source,work,payload,now,turn]).map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Read saved activity durations for the exact bound native thread and item identities.
	pub async fn read_agent_activity_durations(
		&self,
		work: String,
		thread: String,
		items: Vec<(String, String)>,
	) -> Result<Vec<(String, String, u64)>, StoreError> {
		if items.len() > 100
			|| items.iter().any(|(turn, item)| turn.len() > 512 || item.len() > 512)
		{
			return Err(StoreError::InvalidInput("invalid activity identities"));
		}
		let items = serde_json::to_string(&items).expect("serializable identities");
		self.run(move |connection| {
			connection.prepare("SELECT json_extract(requested.value,'$[0]'),json_extract(requested.value,'$[1]'),json_extract(e.payload,'$.duration_ms') FROM json_each(?3) requested JOIN agent_inbox_events e ON e.source_event_id=json_array('activity',?1,json_extract(requested.value,'$[0]'),json_extract(requested.value,'$[1]'),'completed') JOIN agent_work_items w ON w.id=e.work_item_id WHERE w.id=?1 AND w.codex_thread_id=?2 AND json_type(e.payload,'$.duration_ms')='integer' AND json_extract(e.payload,'$.duration_ms')>=0")
				.map_err(sqlite_error)?.query_map(params![work,thread,items], |row| Ok((row.get(0)?,row.get(1)?,row.get::<_, i64>(2)? as u64)))
				.map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error| sqlite_error(error).into())
		}).await
	}

	/// Append an immutable activity receipt without changing work status or waking an agent.
	pub async fn record_agent_activity(
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
		// Native subagent and yielded MCP observations can outlive their originating turn.
		// Only an exact saved terminal receipt can authorize that historical association.
		let historical_activity =
			serde_json::from_str::<serde_json::Value>(&payload).is_ok_and(|value| {
				matches!(value["kind"].as_str(), Some("subAgentActivity" | "mcpToolCall"))
					&& value["turn_id"] == turn
					&& value["item_id"] == item
			});
		let terminal_source = serde_json::json!(["turn/completed", thread, turn]).to_string();
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work:Option<String>=tx.query_row("SELECT w.id FROM agent_work_items w WHERE w.codex_thread_id=?1 AND ((w.active_turn_id=?2 AND w.dispatch_state='running') OR (?3 AND EXISTS(SELECT 1 FROM agent_inbox_events e WHERE e.work_item_id=w.id AND e.source_event_id=?4 AND e.event_kind IN ('agent_turn_completed','worker_turn_completed','capacity_retry'))))",params![thread,turn,historical_activity,terminal_source],|row|row.get(0)).optional().map_err(sqlite_error)?;
			let Some(work)=work else { return Ok(()); };
			let count:i64=tx.query_row("SELECT count(*) FROM agent_inbox_events WHERE work_item_id=?1 AND delivered_turn_id=?2 AND event_kind IN ('activity_started','activity_completed')",params![work,turn],|row|row.get(0)).map_err(sqlite_error)?;
			if count>=256 { return Ok(()); }
			let stage=if completed {"completed"} else {"started"};
			let source=serde_json::json!(["activity",work,turn,item,stage]).to_string();
			let payload = if completed { activity_duration(&tx, &work, &turn, &item, payload)? } else { payload };
			let now=crate::unix_micros()?;
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,?3,?4,?5,'resolved','Observed execution activity',?5,?2,?6) ON CONFLICT(source_event_id) DO NOTHING",params![source,work,format!("activity_{stage}"),payload,now,turn]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}
}

// Use the native lifecycle clock, never receipt arrival time. Keep the original
// receipts immutable and leave old or incomplete histories without an estimate.
fn activity_duration(
	tx: &rusqlite::Transaction<'_>,
	work: &str,
	turn: &str,
	item: &str,
	payload: String,
) -> Result<String, StoreError> {
	let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else {
		return Ok(payload);
	};
	let Some(end) = value["native_timestamp_ms"].as_u64() else {
		return Ok(payload);
	};
	if value["duration_ms"].as_u64().is_some() {
		return Ok(payload);
	}
	let source = serde_json::json!(["activity", work, turn, item, "started"]).to_string();
	let start: Option<String> = tx
		.query_row(
			"SELECT payload FROM agent_inbox_events WHERE source_event_id=?1",
			params![source],
			|row| row.get(0),
		)
		.optional()
		.map_err(sqlite_error)?;
	let start = start
		.and_then(|payload| serde_json::from_str::<serde_json::Value>(&payload).ok())
		.and_then(|value| value["native_timestamp_ms"].as_u64());
	if let Some(duration) = start.and_then(|start| end.checked_sub(start)) {
		value["duration_ms"] = serde_json::json!(duration);
		return Ok(value.to_string());
	}
	Ok(payload)
}

impl SqliteStore {
	/// Save the latest observed checklist as an immutable, non-waking receipt.
	pub async fn record_agent_checklist(
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
            let work: Option<String> = tx.query_row("SELECT id FROM agent_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'",params![thread,turn],|row|row.get(0)).optional().map_err(sqlite_error)?;
            let Some(work) = work else { return Ok(()); };
            let previous: Option<(i64,String)> = tx.query_row("SELECT id,payload FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='plan_updated' AND delivered_turn_id=?2 AND json_extract(payload,'$.threadId')=?3 ORDER BY id DESC LIMIT 1",params![work,turn,thread],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(sqlite_error)?;
            let original_payload = serde_json::json!({"threadId":thread,"turnId":turn,"text":text}).to_string();
            if previous.as_ref().is_some_and(|(_,prior)|prior == &original_payload) { return Ok(()); }
            let count: i64 = tx.query_row("SELECT count(*) FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='plan_updated' AND delivered_turn_id=?2 AND json_extract(payload,'$.threadId')=?3",params![work,turn,thread],|row|row.get(0)).map_err(sqlite_error)?;
            if count > 128 { return Ok(()); }
            let text = if count == 128 { "Checklist update limit reached. Later step states are unavailable.".to_owned() } else { text };
            let payload = serde_json::json!({"threadId":thread,"turnId":turn,"text":text}).to_string();
            let source = serde_json::json!(["plan_updated",thread,turn,previous.map(|(id,_)|id)]).to_string();
            let now = crate::unix_micros()?;
            tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'plan_updated',?3,?4,'resolved','Observed native checklist',?4,?2,?5)",params![source,work,payload,now,turn]).map_err(sqlite_error)?;
            tx.commit().map_err(sqlite_error)?;
            Ok(())
        }).await
	}
}

fn insert_warning(
	tx: &rusqlite::Transaction<'_>,
	work: &str,
	generation: &str,
	digest: &str,
	text: &str,
	kind: &str,
) -> Result<(), StoreError> {
	let original = serde_json::json!([kind, work, generation, digest]).to_string();
	if tx
		.query_row(
			"SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE source_event_id=?1 OR
                (work_item_id=?2 AND event_kind IN ('config_warning','native_warning')
                 AND json_extract(payload,'$.generation')=?3
                 AND json_extract(payload,'$.text')=?4))",
			params![original, work, generation, text],
			|r| r.get::<_, bool>(0),
		)
		.map_err(sqlite_error)?
	{
		return Ok(());
	}
	let count:i64 = tx.query_row("SELECT count(*) FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind=?2 AND json_extract(payload,'$.generation')=?3",params![work,kind,generation],|r|r.get(0)).map_err(sqlite_error)?;
	let (digest, text) = if count >= 64 {
		(
			"overflow",
			if kind == "config_warning" {
				"Additional configuration warnings exceeded the display limit."
			} else {
				"Additional Codex warnings exceeded the display limit."
			},
		)
	} else {
		(digest, text)
	};
	let source = serde_json::json!([kind, work, generation, digest]).to_string();
	let payload = serde_json::json!({"generation":generation,"text":text}).to_string();
	let now = crate::unix_micros()?;
	tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,?3,?4,?5,'resolved','Observed native warning',?5) ON CONFLICT(source_event_id) DO NOTHING",params![source,work,kind,payload,now]).map_err(sqlite_error)?;
	Ok(())
}

/// Preserve unfinished display text in the same transaction as its terminal receipt.
/// These resolved records cannot wake a manager or serve as worker result evidence.
pub(crate) fn retain_partial_output(
	tx: &rusqlite::Transaction<'_>,
	work: &str,
	thread: &str,
	turn: &str,
) -> Result<(), StoreError> {
	let rows = tx.prepare("SELECT item_id,kind,text,truncated FROM agent_live_output WHERE work_id=?1 AND turn_id=?2 AND completed=0 AND kind IN ('agentMessage','plan') ORDER BY id LIMIT 32")
        .map_err(sqlite_error)?.query_map(params![work,turn], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,bool>(3)?)))
        .map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?;
	for (item, kind, text, truncated) in rows {
		if text.is_empty() {
			continue;
		}
		let source = serde_json::json!(["partial_output", thread, turn, item]).to_string();
		let mut value = serde_json::json!({"threadId":thread,"turnId":turn,"itemId":item,"kind":kind,"text":text,"truncated":truncated});
		// Include JSON escaping in the existing inbox payload bound.
		while value.to_string().len() > 65536 {
			let text = value["text"].as_str().unwrap_or_default();
			let mut end = text.len().saturating_sub(1024);
			while !text.is_char_boundary(end) {
				end -= 1;
			}
			value["text"] = text[..end].into();
			value["truncated"] = true.into();
		}
		let now = crate::unix_micros()?;
		tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'partial_output',?3,?4,'resolved','Display-only unfinished output',?4,?2,?5) ON CONFLICT(source_event_id) DO NOTHING",params![source,work,value.to_string(),now,turn]).map_err(sqlite_error)?;
	}
	Ok(())
}
