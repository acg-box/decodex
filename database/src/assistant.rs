//! Assistant records reuse the indexed Inbox journal. Native actions have separate receipts.
use decodex_core::assistant::{AssistantCommand, AssistantSettings, AssistantState, AssistantWake};
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{DatabaseError, EnqueueAgentEvent, SqliteStore, StoreError, agent, error};

fn invalid() -> StoreError {
	StoreError::InvalidInput("invalid Assistant command or authority")
}
fn bounded(s: &str, max: usize) -> Result<(), StoreError> {
	if s.trim().is_empty() || s.len() > max {
		return Err(invalid());
	}
	Ok(())
}

fn decode<T: DeserializeOwned>(s: String) -> Result<T, StoreError> {
	serde_json::from_str(&s).map_err(|_| invalid())
}
fn current(c: &Connection, owner: &str, kind: &str, id: &str) -> Result<Option<Value>, StoreError> {
	c.query_row("SELECT payload FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind=?2 AND json_extract(payload,'$.id')=?3 ORDER BY id DESC LIMIT 1",rusqlite::params![owner,kind,id],|r|r.get::<_,String>(0)).optional().map_err(error::sqlite_error)?.map(decode).transpose()
}
fn records<T: DeserializeOwned>(
	c: &Connection,
	owner: &str,
	kind: &str,
) -> Result<Vec<T>, StoreError> {
	let values = c.prepare("SELECT payload FROM (SELECT id,payload,row_number() OVER(PARTITION BY json_extract(payload,'$.id') ORDER BY id DESC) AS latest_rank FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind=?2) WHERE latest_rank=1 AND json_type(payload,'$.value')<>'null' ORDER BY id").map_err(error::sqlite_error)?.query_map(rusqlite::params![owner,kind],|r|r.get::<_,String>(0)).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(error::sqlite_error)?;
	values
		.into_iter()
		.map(|s| {
			let v: Value = decode(s)?;
			serde_json::from_value(v["value"].clone()).map_err(|_| invalid())
		})
		.collect()
}
fn record(
	tx: &Transaction<'_>,
	owner: &str,
	key: &str,
	kind: &str,
	payload: Value,
) -> Result<(), StoreError> {
	agent::insert_event(
		tx,
		EnqueueAgentEvent {
			source_event_id: key.into(),
			work_item_id: owner.into(),
			event_kind: kind.into(),
			payload: payload.to_string(),
		},
		true,
	)?;
	Ok(())
}
pub(crate) fn require_assistant(c: &Connection, owner: &str) -> Result<(), StoreError> {
	if !c
		.query_row(
			"SELECT EXISTS(SELECT 1 FROM agents WHERE id=?1 AND assistant=1)",
			[owner],
			|r| r.get::<_, bool>(0),
		)
		.map_err(error::sqlite_error)?
	{
		return Err(invalid());
	}
	Ok(())
}

pub(crate) fn cancel_wakes_on_archive(
	tx: &Transaction<'_>,
	owner: &str,
	thread: &str,
) -> Result<(), StoreError> {
	for wake in records::<AssistantWake>(tx, owner, "assistant_internal_wake")? {
		if let Some(due) = wake.due_at_micros {
			record(
				tx,
				owner,
				&json!(["assistant_archive_wake", owner, thread, wake.id, due]).to_string(),
				"assistant_internal_wake",
				json!({"id":wake.id,"value":null}),
			)?;
		}
	}
	Ok(())
}
pub(crate) fn authority(c: &Connection, owner: &str, event: i64) -> Result<(), StoreError> {
	let valid=c.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE id=?2 AND work_item_id=?1 AND event_kind IN('user_message','async_question_answer') AND delivered_turn_id IS NOT NULL AND delivered_turn_id<>'' AND (json_extract(payload,'$.source')='user' OR event_kind='async_question_answer'))",rusqlite::params![owner,event],|r|r.get::<_,bool>(0)).map_err(error::sqlite_error)?;
	if !valid {
		return Err(invalid());
	}
	Ok(())
}
pub(crate) fn settings(c: &Connection, owner: &str) -> Result<AssistantSettings, StoreError> {
	current(c, owner, "assistant_internal_settings", "settings")?
		.map(|v| serde_json::from_value(v["value"].clone()).map_err(|_| invalid()))
		.transpose()
		.map(|s| s.unwrap_or_default())
}

fn configure(
	tx: &Transaction<'_>,
	owner: &str,
	receipt: &str,
	s: &AssistantSettings,
	user: bool,
) -> Result<(), StoreError> {
	if !user || s.mandate.len() > 16_384 || s.renew_after_turns > 200 {
		return Err(invalid());
	}
	if let Some(r) = &s.ragflow {
		bounded(&r.url, 2_048)?;
		bounded(&r.credential_id, 128)?;
		bounded(&r.user_id, 512)?;
		if r.dataset_ids.len() > 32
			|| r.dataset_ids.iter().any(|id| bounded(id, 128).is_err())
			|| r.memory_id.as_ref().is_some_and(|id| bounded(id, 128).is_err())
			|| (r.ingest_chats && r.memory_id.is_none())
		{
			return Err(invalid());
		}
	}
	let before = settings(tx, owner)?;
	if before.ragflow.as_ref().map(|r| (&r.url, &r.memory_id, &r.user_id))
		!= s.ragflow.as_ref().map(|r| (&r.url, &r.memory_id, &r.user_id))
	{
		record(
			tx,
			owner,
			&format!("{receipt}:memory-health"),
			"assistant_internal_memory_health",
			json!({"id":"health","value":null}),
		)?;
	}
	let export = s.ragflow.as_ref().filter(|r| r.ingest_chats);
	if export.is_some()
		&& before
			.ragflow
			.as_ref()
			.filter(|r| r.ingest_chats)
			.map(|r| (&r.url, &r.memory_id, &r.user_id))
			!= export.map(|r| (&r.url, &r.memory_id, &r.user_id))
	{
		record(
			tx,
			owner,
			&format!("{receipt}:export"),
			"assistant_internal_export_boundary",
			json!({"enabled":true}),
		)?;
	}
	Ok(())
}

pub(crate) fn restore_configuration_after_deletion(
	tx: &Transaction<'_>,
	owner: &str,
	thread: &str,
	saved: &AssistantSettings,
) -> Result<(), StoreError> {
	let key = json!(["assistant_deleted_configuration", owner, thread]).to_string();
	configure(tx, owner, &key, saved, true)?;
	record(tx, owner, &key, "assistant_internal_settings", json!({"id":"settings","value":saved}))
}

impl SqliteStore {
	pub async fn assistant_state(&self) -> Result<AssistantState, StoreError> {
		self.run(|c| {
			let owner=c.query_row("SELECT id FROM agents WHERE assistant=1",[],|r|r.get::<_,String>(0)).optional().map_err(error::sqlite_error)?;
			let Some(owner)=owner else {return Ok(AssistantState::default());};
			let previous=c.prepare("SELECT old_thread_id FROM agent_thread_revisions WHERE work_id=?1 ORDER BY created_at_micros DESC,rowid DESC").map_err(error::sqlite_error)?.query_map([&owner],|r|r.get(0)).map_err(error::sqlite_error)?.collect::<Result<_,_>>().map_err(error::sqlite_error)?;
			let pending=c.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events e JOIN agent_work_items w ON w.id=e.work_item_id WHERE w.id=?1 AND e.source_event_id=json_array('assistant_internal_context',w.id,w.codex_thread_id))",[&owner],|r|r.get(0)).map_err(error::sqlite_error)?;
			let memory_status=current(c,&owner,"assistant_internal_memory_health","health")?.and_then(|v|v["value"].as_str().map(str::to_owned));
			Ok(AssistantState {settings:settings(c,&owner)?,teams:records(c,&owner,"assistant_internal_team")?,wakes:records(c,&owner,"assistant_internal_wake")?,previous_thread_ids:previous,context_pending:pending,memory_status,agent_id:Some(owner)})
		}).await
	}

	pub async fn assistant_settings(
		&self,
		owner: String,
	) -> Result<Option<AssistantSettings>, StoreError> {
		self.run(move |c| {
			if require_assistant(c, &owner).is_err() {
				return Ok(None);
			}
			settings(c, &owner).map(Some)
		})
		.await
	}

	/// Save application state only. Persistent creation is handled by the conversation owner.
	pub async fn apply_assistant_command(
		&self,
		owner: String,
		key: String,
		command: AssistantCommand,
		user: bool,
	) -> Result<(), StoreError> {
		bounded(&key, 512)?;
		self.run(move |c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;
			require_assistant(&tx, &owner)?;
			let receipt = json!(["assistant_internal_command", owner, key]).to_string();
			let encoded = json!({"id":key,"command":command});
			let prior = tx
				.query_row(
					"SELECT payload FROM agent_inbox_events WHERE source_event_id=?1",
					[&receipt],
					|r| r.get::<_, String>(0),
				)
				.optional()
				.map_err(error::sqlite_error)?;
			if let Some(prior) = prior {
				return if decode::<Value>(prior)? == encoded {
					Ok(())
				} else {
					Err(StoreError::IdempotencyConflict)
				};
			}
			let (kind, id, value) = match &command {
				AssistantCommand::Configure { settings: s } => {
					configure(&tx, &owner, &receipt, s, user)?;
					("assistant_internal_settings", "settings".to_owned(), json!(s))
				},
				AssistantCommand::SetMandate { text, authority_event_id } => {
					if !user {
						authority(&tx, &owner, *authority_event_id)?;
					}
					bounded(text, 16_384)?;
					let mut s = settings(&tx, &owner)?;
					s.mandate = text.clone();
					("assistant_internal_settings", "settings".into(), json!(s))
				},
				AssistantCommand::PutTeam { team } => {
					bounded(&team.id, 128)?;
					bounded(&team.name, 256)?;
					if team.members.len() > 100 {
						return Err(invalid());
					}
					let mut seen = std::collections::HashSet::new();
					for m in &team.members {
						bounded(&m.responsibility, 2_048)?;
						if !seen.insert(&m.agent_id)
							|| !tx
								.query_row(
									"SELECT EXISTS(SELECT 1 FROM agents WHERE id=?1)",
									[&m.agent_id],
									|r| r.get::<_, bool>(0),
								)
								.map_err(error::sqlite_error)?
						{
							return Err(invalid());
						}
					}
					if let Some(p) = &team.project_id
						&& !tx
							.query_row(
								"SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
								[p],
								|r| r.get::<_, bool>(0),
							)
							.map_err(error::sqlite_error)?
					{
						return Err(invalid());
					}
					("assistant_internal_team", team.id.clone(), json!(team))
				},
				AssistantCommand::RemoveTeam { team_id } =>
					("assistant_internal_team", team_id.clone(), Value::Null),
				AssistantCommand::Schedule { wake } => {
					bounded(&wake.id, 128)?;
					bounded(&wake.instruction, 8_192)?;
					if !user {
						authority(&tx, &owner, wake.authority_event_id)?;
					}
					if !wake
						.due_at_micros
						.is_some_and(|due| due > crate::unix_micros().unwrap_or(i64::MAX))
						|| wake.interval_seconds.is_some_and(|s| s < 60)
					{
						return Err(invalid());
					}
					("assistant_internal_wake", wake.id.clone(), json!(wake))
				},
				AssistantCommand::CancelWake { wake_id } =>
					("assistant_internal_wake", wake_id.clone(), Value::Null),
				AssistantCommand::RefreshContext =>
					("assistant_internal_refresh", "refresh".into(), json!(true)),
				AssistantCommand::CreateAgent { .. } => return Err(invalid()),
			};
			if matches!(
				&command,
				AssistantCommand::Configure { .. } | AssistantCommand::SetMandate { .. }
			) {
				record(
					&tx,
					&owner,
					&format!("{receipt}:refresh"),
					"assistant_internal_refresh",
					json!({"id":"refresh","value":true}),
				)?;
			}
			record(&tx, &owner, &format!("{receipt}:state"), kind, json!({"id":id,"value":value}))?;
			record(&tx, &owner, &receipt, "assistant_internal_command", encoded)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(())
		})
		.await
	}

	/// Claim due occurrences and queue their stable Agent-owned evidence in one transaction.
	pub async fn enqueue_assistant_wakes(&self, now: i64) -> Result<(), StoreError> {
		self.run(move|c| {
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let owner=tx.query_row("SELECT id FROM agents WHERE assistant=1",[],|r|r.get::<_,String>(0)).optional().map_err(error::sqlite_error)?;
			if let Some(owner)=owner {
				for mut wake in records::<AssistantWake>(&tx,&owner,"assistant_internal_wake")? {
					let Some(due)=wake.due_at_micros.filter(|due|*due<=now) else {continue;};
					let key=json!(["assistant_wake_due",owner,wake.id,due]).to_string();
					agent::insert_event(&tx,EnqueueAgentEvent {source_event_id:key.clone(),work_item_id:owner.clone(),event_kind:"followup_due".into(),payload:json!({"wakeId":wake.id,"instruction":wake.instruction,"authorityEventId":wake.authority_event_id,"dueAtMicros":due,"notificationPolicy":"Notify only on a meaningful change, result, failure or required user action."}).to_string()},false)?;
					wake.due_at_micros=wake.interval_seconds.and_then(|s|now.checked_add(i64::from(s)*1_000_000));
					record(&tx,&owner,&format!("{key}:advance"),"assistant_internal_wake",json!({"id":wake.id,"value":wake}))?;
				}
			}
			tx.commit().map_err(error::sqlite_error)?;Ok(())
		}).await
	}

	/// The durable creation receipt is retained before naming, settings or history reads.
	pub async fn assistant_context_receipt(
		&self,
		work: String,
		old: String,
	) -> Result<Option<Value>, StoreError> {
		self.run(move|c| c.query_row("SELECT json_set(e.payload,'$.newThreadId',json_extract(a.payload,'$.newThreadId')) FROM agent_inbox_events e LEFT JOIN agent_inbox_events a ON a.source_event_id=json_array('assistant_internal_context_ack',?1,?2) WHERE e.source_event_id=json_array('assistant_internal_context',?1,?2)",rusqlite::params![work,old],|r|r.get::<_,String>(0)).optional().map_err(error::sqlite_error)?.map(decode).transpose()).await
	}

	pub async fn reserve_assistant_context(
		&self,
		work: String,
		old: String,
		bootstrap: Value,
	) -> Result<bool, StoreError> {
		self.run(move|c| {
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			require_assistant(&tx,&work)?;
			let idle=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id=?1 AND codex_thread_id=?2 AND dispatch_state='idle' AND active_turn_id IS NULL) AND NOT EXISTS(SELECT 1 FROM agent_voice_calls WHERE work_id=?1 AND closed_at_micros IS NULL) AND NOT EXISTS(SELECT 1 FROM agent_capacity_retries WHERE work_item_id=?1 AND state IN('pending','claimed')) AND NOT EXISTS(SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 AND disposition IS NULL AND event_kind IN('permission_pending','user_input_pending','server_request_pending','async_question_answer','steer_pending')) AND NOT EXISTS(SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 AND disposition IS NULL AND delivered_turn_id='')",rusqlite::params![work,old],|r|r.get::<_,bool>(0)).map_err(error::sqlite_error)?;
			if !idle {return Ok(false);}
			let key=json!(["assistant_internal_context",work,old]).to_string();
			if tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE source_event_id=?1)",[&key],|r|r.get::<_,bool>(0)).map_err(error::sqlite_error)? {return Err(DatabaseError::Conflict.into());}
			record(&tx,&work,&key,"assistant_internal_context",json!({"oldThreadId":old,"newThreadId":null,"bootstrap":bootstrap}))?;
			tx.commit().map_err(error::sqlite_error)?;Ok(true)
		}).await
	}

	pub async fn acknowledge_assistant_context(
		&self,
		work: String,
		old: String,
		new: String,
	) -> Result<(), StoreError> {
		self.run(move|c| {
            let tx=c.transaction().map_err(error::sqlite_error)?;
            if !tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE source_event_id=json_array('assistant_internal_context',?1,?2))",rusqlite::params![work,old],|r|r.get::<_,bool>(0)).map_err(error::sqlite_error)? {return Err(invalid());}
            record(&tx,&work,&json!(["assistant_internal_context_ack",work,old]).to_string(),"assistant_internal_context_ack",json!({"newThreadId":new}))?;
            tx.commit().map_err(error::sqlite_error)?;Ok(())
        }).await
	}

	pub async fn activate_assistant_context(
		&self,
		work: String,
		old: String,
		new: String,
	) -> Result<(), StoreError> {
		self.run(move|c| {
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let receipt=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE source_event_id=json_array('assistant_internal_context_ack',?1,?2) AND json_extract(payload,'$.newThreadId')=?3)",rusqlite::params![work,old,new],|r|r.get::<_,bool>(0)).map_err(error::sqlite_error)?;
			if !receipt {return Err(invalid());}
			let now=crate::unix_micros()?;
			let n=tx.execute("UPDATE agent_work_items SET codex_thread_id=?3,updated_at_micros=?4 WHERE id=?1 AND codex_thread_id=?2 AND dispatch_state='idle' AND active_turn_id IS NULL",rusqlite::params![work,old,new,now]).map_err(error::sqlite_error)?;
			if n!=1 {return Err(DatabaseError::Conflict.into());}
			tx.execute("INSERT INTO agent_thread_revisions(work_id,old_thread_id,new_thread_id,created_at_micros) VALUES(?1,?2,?3,?4)",rusqlite::params![work,old,new,now]).map_err(error::sqlite_error)?;
			record(&tx,&work,&json!(["assistant_refresh_done",work,new]).to_string(),"assistant_internal_refresh",json!({"id":"refresh","value":false}))?;
			tx.commit().map_err(error::sqlite_error)?;Ok(())
		}).await
	}

	pub async fn assistant_refresh_due(
		&self,
		work: String,
		thread: String,
		turns: u32,
	) -> Result<bool, StoreError> {
		self.run(move|c| {
			let requested=current(c,&work,"assistant_internal_refresh","refresh")?.is_some_and(|v|v["value"]==true);
			let count=c.query_row("SELECT count(*) FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind IN('agent_turn_completed','worker_turn_completed') AND json_extract(payload,'$.terminal.threadId')=?2",rusqlite::params![work,thread],|r|r.get::<_,u32>(0)).map_err(error::sqlite_error)?;
			let pressure=c.query_row("SELECT coalesce(json_extract(usage_json,'$.context_tokens')*5>=json_extract(usage_json,'$.context_window')*4,0) FROM agent_usage WHERE work_id=?1 AND thread_id=?2",rusqlite::params![work,thread],|r|r.get::<_,bool>(0)).optional().map_err(error::sqlite_error)?.unwrap_or(false);
			Ok(requested || (turns>0 && (count>=turns || pressure)))
		}).await
	}

	pub async fn save_assistant_bootstrap(
		&self,
		work: String,
		thread: String,
		bootstrap: Value,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(error::sqlite_error)?;
			record(
				&tx,
				&work,
				&json!(["assistant_internal_bootstrap", work, thread]).to_string(),
				"assistant_internal_bootstrap",
				bootstrap,
			)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(())
		})
		.await
	}

	pub async fn assistant_external_memory_context(
		&self,
		work: String,
		thread: String,
	) -> Result<bool, StoreError> {
		self.run(move|c| c.query_row("SELECT coalesce(json_extract(payload,'$.externalMemory'),0) FROM agent_inbox_events WHERE source_event_id=json_array('assistant_internal_bootstrap',?1,?2)",rusqlite::params![work,thread],|r|r.get(0)).optional().map(|v|v.unwrap_or(false)).map_err(|e|error::sqlite_error(e).into())).await
	}

	pub async fn assistant_bootstrap(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<Value>, StoreError> {
		self.run(move|c|c.query_row("SELECT payload FROM agent_inbox_events WHERE source_event_id=json_array('assistant_internal_bootstrap',?1,?2) AND NOT EXISTS(SELECT 1 FROM agent_inbox_events WHERE source_event_id=json_array('assistant_internal_bootstrap_sent',?1,?2))",rusqlite::params![work,thread],|r|r.get::<_,String>(0)).optional().map_err(error::sqlite_error)?.map(decode).transpose()).await
	}

	pub async fn mark_assistant_bootstrap_sent(
		&self,
		work: String,
		thread: String,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(error::sqlite_error)?;
			record(
				&tx,
				&work,
				&json!(["assistant_internal_bootstrap_sent", work, thread]).to_string(),
				"assistant_internal_bootstrap_sent",
				json!({"threadId":thread}),
			)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(())
		})
		.await
	}

	pub async fn reserve_assistant_memory(
		&self,
		owner: String,
		key: String,
		request: Value,
	) -> Result<bool, StoreError> {
		self.run(move |c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;
			require_assistant(&tx, &owner)?;
			let source = json!(["assistant_internal_memory", owner, key]).to_string();
			if let Some(saved) = tx
				.query_row(
					"SELECT payload FROM agent_inbox_events WHERE source_event_id=?1",
					[&source],
					|r| r.get::<_, String>(0),
				)
				.optional()
				.map_err(error::sqlite_error)?
			{
				if decode::<Value>(saved)?["request"] != request {
					return Err(StoreError::IdempotencyConflict);
				}
				return Ok(false);
			}
			record(
				&tx,
				&owner,
				&source,
				"assistant_internal_memory",
				json!({"request":request,"state":"unknown"}),
			)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(true)
		})
		.await
	}

	pub async fn acknowledge_assistant_memory(
		&self,
		owner: String,
		key: String,
		state: String,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(error::sqlite_error)?;
			record(
				&tx,
				&owner,
				&json!(["assistant_internal_memory_result", owner, key]).to_string(),
				"assistant_internal_memory_result",
				json!({"state":state}),
			)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(())
		})
		.await
	}

	pub async fn assistant_memory_receipts(&self, owner: String) -> Result<Vec<Value>, StoreError> {
		self.run(move|c|c.prepare("SELECT json_set(e.payload,'$.state',coalesce(json_extract(r.payload,'$.state'),'unknown')) FROM agent_inbox_events e LEFT JOIN agent_inbox_events r ON r.source_event_id=json_array('assistant_internal_memory_result',e.work_item_id,json_extract(e.source_event_id,'$[2]')) WHERE e.work_item_id=?1 AND e.event_kind='assistant_internal_memory' ORDER BY e.id DESC LIMIT 20").map_err(error::sqlite_error)?.query_map([owner],|r|r.get::<_,String>(0)).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(error::sqlite_error)?.into_iter().map(decode).collect()).await
	}

	pub async fn assistant_export_turn(
		&self,
		owner: String,
		memory: String,
	) -> Result<Option<(String, String)>, StoreError> {
		// Match receipt fields: the operation key is a serialized JSON string, not a nested array.
		self.run(move|c| c.query_row("SELECT json_extract(e.payload,'$.terminal.threadId'),json_extract(e.payload,'$.terminal.turn.id') FROM agent_inbox_events e WHERE work_item_id=?1 AND event_kind='agent_turn_completed' AND json_extract(payload,'$.terminal.turn.status')='completed' AND id>coalesce((SELECT max(id) FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='assistant_internal_export_boundary'),9223372036854775807) AND NOT EXISTS(SELECT 1 FROM agent_inbox_events m WHERE m.work_item_id=?1 AND m.event_kind='assistant_internal_memory' AND json_extract(m.payload,'$.request.operation')='export' AND json_extract(m.payload,'$.request.memoryId')=?2 AND json_extract(m.payload,'$.request.threadId')=json_extract(e.payload,'$.terminal.threadId') AND json_extract(m.payload,'$.request.turnId')=json_extract(e.payload,'$.terminal.turn.id')) AND NOT EXISTS(SELECT 1 FROM agent_inbox_events m JOIN agent_inbox_events a ON a.id=json_extract(m.payload,'$.request.authorityEventId') WHERE m.work_item_id=?1 AND m.event_kind='assistant_internal_memory' AND json_extract(m.payload,'$.request.memoryId')=?2 AND json_extract(m.payload,'$.request.operation') IN('remember','forget') AND a.delivered_turn_id=json_extract(e.payload,'$.terminal.turn.id')) ORDER BY id LIMIT 1",rusqlite::params![owner,memory],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|error::sqlite_error(e).into())).await
	}

	pub async fn assistant_verify_authority(
		&self,
		owner: String,
		event: i64,
	) -> Result<(), StoreError> {
		self.run(move |c| authority(c, &owner, event)).await
	}

	pub async fn assistant_context_metadata(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<Value>, StoreError> {
		self.run(move|c|c.query_row("SELECT payload FROM agent_inbox_events WHERE source_event_id=json_array('assistant_internal_bootstrap',?1,?2)",rusqlite::params![work,thread],|r|r.get::<_,String>(0)).optional().map_err(error::sqlite_error)?.map(decode).transpose()).await
	}

	pub async fn previous_agent_context(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<String>, StoreError> {
		self.run(move |c| {
			c.query_row(
				"SELECT old_thread_id FROM agent_thread_revisions WHERE work_id=?1 AND new_thread_id=?2",
				rusqlite::params![work, thread],
				|r| r.get(0),
			)
			.optional()
			.map_err(|e| error::sqlite_error(e).into())
		})
		.await
	}

	pub async fn assistant_memory_health(
		&self,
		owner: String,
		source: decodex_core::assistant::RagflowConnection,
		status: String,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(error::sqlite_error)?;
			if settings(&tx, &owner)?.ragflow.as_ref() != Some(&source) {
				return Ok(());
			}
			if current(&tx, &owner, "assistant_internal_memory_health", "health")?
				.is_some_and(|v| v["value"] == status)
			{
				return Ok(());
			}
			let previous: i64 = tx.query_row(
				"SELECT coalesce(max(id),0) FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='assistant_internal_memory_health'",
				[&owner], |r| r.get(0),
			).map_err(error::sqlite_error)?;
			record(&tx, &owner, &json!(["assistant_internal_memory_health",owner,previous]).to_string(), "assistant_internal_memory_health", json!({"id":"health","value":status}))?;
			tx.commit().map_err(error::sqlite_error)?;

			Ok(())
		}).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus};
	use decodex_core::assistant::{AgentTeam, TeamMember};

	async fn agent(store: &SqliteStore, id: &str) -> AgentWorkItem {
		let item = AgentWorkItem {
			id: id.into(),
			parent_goal_id: None,
			kind: AgentWorkKind::Goal,
			title: id.into(),
			instructions: "Help with authorized work".into(),
			codex_thread_id: None,
			status: AgentWorkStatus::Open,
			dispatch_state: AgentDispatchState::Idle,
			active_turn_id: None,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		};
		store.create_agent_manager(item.clone()).await.unwrap();
		item
	}
	#[tokio::test]
	async fn assistant_setup_is_idle_atomic_and_safe_to_repeat() {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("setup.sqlite3")).unwrap();
		let mut item = agent(&store, "existing").await;
		store.set_assistant(item.id.clone(), true).await.unwrap();
		item.id = "new-assistant".into();
		item.status = AgentWorkStatus::Wait;
		let first = store.create_assistant(item.clone()).await.unwrap();
		let repeated = store.create_assistant(item).await.unwrap();
		assert_eq!(first, repeated);
		assert!(repeated.codex_thread_id.is_none());
		assert!(repeated.active_turn_id.is_none());
		assert_eq!(repeated.dispatch_state, AgentDispatchState::Idle);
		assert_eq!(store.list_agent_work_items().await.unwrap().len(), 2);
		let agents = store.agents().await.unwrap();
		assert_eq!(agents.iter().filter(|a| a.assistant).count(), 1);
		assert_eq!(
			store.assistant_state().await.unwrap().agent_id.as_deref(),
			Some("new-assistant")
		);
		let events = store
			.run(|c| {
				c.query_row("SELECT count(*) FROM agent_inbox_events", [], |r| r.get::<_, i64>(0))
					.map_err(|e| error::sqlite_error(e).into())
			})
			.await
			.unwrap();
		assert_eq!(events, 0, "setup must not manufacture a first user message");
	}

	async fn setup() -> (tempfile::TempDir, SqliteStore, i64) {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("assistant.sqlite3")).unwrap();
		agent(&store, "assistant").await;
		store.set_assistant("assistant".into(), true).await.unwrap();
		store.begin_agent_thread_creation("assistant".into()).await.unwrap();
		store.acknowledge_agent_thread_creation("assistant".into(), "old".into()).await.unwrap();
		let event = store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "user-authority".into(),
				work_item_id: "assistant".into(),
				event_kind: "user_message".into(),
				payload: json!({"source":"user","text":"Manage the team and follow these duties"})
					.to_string(),
			})
			.await
			.unwrap();
		store.begin_agent_dispatch_with_events("assistant".into(), vec![event.id]).await.unwrap();
		store.acknowledge_agent_dispatch("assistant".into(), "decision-turn".into()).await.unwrap();
		store.complete_agent_turn("assistant".into(), "decision-turn".into()).await.unwrap();
		(dir, store, event.id)
	}

	#[tokio::test]
	async fn assistant_teams_share_members_and_removal_does_not_cancel_agents() {
		let (_dir, store, _) = setup().await;
		agent(&store, "member").await;
		let team = |id: &str| AssistantCommand::PutTeam {
			team: AgentTeam {
				id: id.into(),
				name: id.into(),
				project_id: None,
				members: vec![TeamMember {
					agent_id: "member".into(),
					responsibility: "Research".into(),
				}],
			},
		};
		for id in ["one", "two"] {
			store
				.apply_assistant_command("assistant".into(), id.into(), team(id), false)
				.await
				.unwrap();
		}
		assert_eq!(store.assistant_state().await.unwrap().teams.len(), 2);
		store
			.apply_assistant_command(
				"assistant".into(),
				"remove".into(),
				AssistantCommand::RemoveTeam { team_id: "one".into() },
				false,
			)
			.await
			.unwrap();
		assert_eq!(store.assistant_state().await.unwrap().teams.len(), 1);
		assert!(store.get_agent_work_item("member".into()).await.is_ok());
		assert!(
			store
				.apply_assistant_command("member".into(), "other".into(), team("three"), false)
				.await
				.is_err()
		);
	}

	#[tokio::test]
	async fn assistant_wakes_survive_restart_and_coalesce_without_overwriting_other_duties() {
		let (dir, store, event) = setup().await;
		let now = crate::unix_micros().unwrap();
		for id in ["one", "two"] {
			let command = AssistantCommand::Schedule {
				wake: AssistantWake {
					id: id.into(),
					instruction: format!("Inspect {id}"),
					authority_event_id: event,
					due_at_micros: Some(now + 1_000_000),
					interval_seconds: Some(60),
				},
			};
			store
				.apply_assistant_command("assistant".into(), id.into(), command.clone(), false)
				.await
				.unwrap();
			store
				.apply_assistant_command("assistant".into(), id.into(), command, false)
				.await
				.unwrap();
		}
		drop(store);
		let store = SqliteStore::open_test(&dir.path().join("assistant.sqlite3")).unwrap();
		store.enqueue_assistant_wakes(now + 10_000_000).await.unwrap();
		store.enqueue_assistant_wakes(now + 10_000_000).await.unwrap();
		let wakes = store.list_agent_wake_events("assistant".into(), 100).await.unwrap();
		assert_eq!(wakes.iter().filter(|e| e.event_kind == "followup_due").count(), 2);
		assert_eq!(store.assistant_state().await.unwrap().wakes.len(), 2);
		store.retire_archived_agent_threads(vec!["old".into()]).await.unwrap();
		assert!(store.assistant_state().await.unwrap().wakes.is_empty());
	}

	#[tokio::test]
	async fn assistant_context_receipts_preserve_input_and_never_reserve_creation_twice() {
		let (_dir, store, _) = setup().await;
		let pending = store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: "new-input".into(),
				work_item_id: "assistant".into(),
				event_kind: "user_message".into(),
				payload: json!({"source":"user","text":"Use the corrected requirement"})
					.to_string(),
			})
			.await
			.unwrap();
		let bootstrap = json!({"recentDialogue":"Original correction","externalMemory":true,"timelineOffset":11});
		store
			.reserve_assistant_context("assistant".into(), "old".into(), bootstrap.clone())
			.await
			.unwrap();
		assert!(
			store
				.reserve_assistant_context("assistant".into(), "old".into(), bootstrap.clone())
				.await
				.is_err()
		);
		assert!(
			store
				.activate_assistant_context("assistant".into(), "old".into(), "new".into())
				.await
				.is_err()
		);
		store
			.acknowledge_assistant_context("assistant".into(), "old".into(), "new".into())
			.await
			.unwrap();
		store.save_assistant_bootstrap("assistant".into(), "new".into(), bootstrap).await.unwrap();
		store
			.activate_assistant_context("assistant".into(), "old".into(), "new".into())
			.await
			.unwrap();
		assert_eq!(
			store.get_agent_work_item("assistant".into()).await.unwrap().codex_thread_id.as_deref(),
			Some("new")
		);
		assert_eq!(store.agent_previous_threads("assistant".into()).await.unwrap(), ["old"]);
		assert!(store.get_agent_inbox_event(pending.id).await.unwrap().delivered_turn_id.is_none());
		assert!(
			store
				.assistant_external_memory_context("assistant".into(), "new".into())
				.await
				.unwrap()
		);
		assert!(
			store.assistant_bootstrap("assistant".into(), "new".into()).await.unwrap().is_some()
		);
		store.mark_assistant_bootstrap_sent("assistant".into(), "new".into()).await.unwrap();
		assert!(
			store.assistant_bootstrap("assistant".into(), "new".into()).await.unwrap().is_none()
		);
	}

	#[tokio::test]
	async fn assistant_delegation_keeps_original_authority_and_exact_creation_identity() {
		let (_dir, store, event) = setup().await;
		let mut item = AgentWorkItem {
			id: "delegated".into(),
			parent_goal_id: None,
			kind: AgentWorkKind::Goal,
			title: "Research".into(),
			instructions: "Inspect the source".into(),
			codex_thread_id: None,
			status: AgentWorkStatus::Wait,
			dispatch_state: AgentDispatchState::Idle,
			active_turn_id: None,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		};
		let input=EnqueueAgentEvent{source_event_id:"delegation".into(),work_item_id:item.id.clone(),event_kind:"assistant_instruction".into(),payload:json!({"source":"assistant","delegatedBy":"assistant","authorityEventId":event,"text":item.instructions}).to_string()};
		store.create_delegated_agent(item.clone(), input.clone(), None).await.unwrap();
		store.create_delegated_agent(item.clone(), input.clone(), None).await.unwrap();
		let events = store.list_agent_wake_events(item.id.clone(), 100).await.unwrap();
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].event_kind, "assistant_instruction");
		let work = decodex_core::work::Work {
			id: "delegated-work".into(),
			project_id: None,
			owner_agent_id: "delegated".into(),
			title: "Inspect source".into(),
			requirements: decodex_core::work::WorkRequirements {
				id: "r1".into(),
				goal: "Inspect the source".into(),
				criteria: "Report source evidence".into(),
				source: format!("event:{}", events[0].id),
			},
			state: decodex_core::work::WorkState::Open,
			reason: None,
			resume_condition: None,
			user_hold: false,
			assessment: None,
		};
		assert!(
			store
				.apply_work_command(
					"delegated".into(),
					"before-delivery".into(),
					decodex_core::work::WorkCommand::Create { work: work.clone() },
					false
				)
				.await
				.is_err()
		);
		store.begin_agent_thread_creation("delegated".into()).await.unwrap();
		store
			.acknowledge_agent_thread_creation("delegated".into(), "member-thread".into())
			.await
			.unwrap();
		store
			.begin_agent_dispatch_with_events("delegated".into(), vec![events[0].id])
			.await
			.unwrap();
		store.acknowledge_agent_dispatch("delegated".into(), "member-turn".into()).await.unwrap();
		store
			.apply_work_command(
				"delegated".into(),
				"create-work".into(),
				decodex_core::work::WorkCommand::Create { work },
				false,
			)
			.await
			.unwrap();
		assert!(
			store.get_agent_work_item("delegated".into()).await.unwrap().parent_goal_id.is_none()
		);
		item.id = "other".into();
		let mut changed = input;
		changed.work_item_id = item.id.clone();
		assert!(store.create_delegated_agent(item, changed, None).await.is_err());
	}
	#[tokio::test]
	async fn assistant_policy_changes_refresh_and_restart_chat_export_scope() {
		use decodex_core::assistant::RagflowConnection;
		let (_dir, store, event) = setup().await;
		assert!(!store.assistant_refresh_due("assistant".into(), "old".into(), 0).await.unwrap());
		store
			.apply_assistant_command(
				"assistant".into(),
				"mandate".into(),
				AssistantCommand::SetMandate {
					text: "Use the corrected continuing instructions".into(),
					authority_event_id: event,
				},
				false,
			)
			.await
			.unwrap();
		assert!(
			store.assistant_refresh_due("assistant".into(), "old".into(), 0).await.unwrap(),
			"changed continuing instructions require a safe fresh context even when periodic renewal is off"
		);
		let connection = |user: &str| AssistantCommand::Configure {
			settings: AssistantSettings {
				mandate: "Current instructions".into(),
				renew_after_turns: 0,
				ragflow: Some(RagflowConnection {
					url: "https://ragflow.example.com".into(),
					credential_id: "a".repeat(64),
					user_id: user.into(),
					dataset_ids: vec![],
					memory_id: Some("dedicated".into()),
					ingest_chats: true,
				}),
			},
		};
		store
			.apply_assistant_command(
				"assistant".into(),
				"scope-a".into(),
				connection("user-a"),
				true,
			)
			.await
			.unwrap();
		store.enqueue_agent_event(EnqueueAgentEvent{source_event_id:"exchange-after-opt-in".into(),work_item_id:"assistant".into(),event_kind:"agent_turn_completed".into(),payload:json!({"terminal":{"threadId":"old","turn":{"id":"completed","status":"completed"}}}).to_string()}).await.unwrap();
		assert!(
			store
				.assistant_export_turn("assistant".into(), "dedicated".into())
				.await
				.unwrap()
				.is_some()
		);
		store
			.apply_assistant_command(
				"assistant".into(),
				"scope-b".into(),
				connection("user-b"),
				true,
			)
			.await
			.unwrap();
		assert!(
			store
				.assistant_export_turn("assistant".into(), "dedicated".into())
				.await
				.unwrap()
				.is_none(),
			"changing the external user namespace must not export earlier pending chat history into that new scope"
		);
	}

	#[tokio::test]
	async fn automatic_export_advances_past_reserved_turns_after_restart() {
		for acknowledgment in [None, Some("queued")] {
			let (dir, store, _) = setup().await;
			store
				.apply_assistant_command(
					"assistant".into(),
					"automatic".into(),
					AssistantCommand::Configure {
						settings: AssistantSettings {
							ragflow: Some(decodex_core::assistant::RagflowConnection {
								url: "https://ragflow.example.test/".into(),
								credential_id: "a".repeat(64),
								user_id: "user".into(),
								dataset_ids: vec![],
								memory_id: Some("personal".into()),
								ingest_chats: true,
							}),
							..Default::default()
						},
					},
					true,
				)
				.await
				.unwrap();
			for turn in ["first", "second"] {
				store.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id:format!("completed-{turn}"),work_item_id:"assistant".into(),event_kind:"agent_turn_completed".into(),
					payload:json!({"terminal":{"threadId":"native","turn":{"id":turn,"status":"completed"}}}).to_string()
				}).await.unwrap();
			}
			for turn in ["first", "second"] {
				let reopened =
					SqliteStore::open_test(&dir.path().join("assistant.sqlite3")).unwrap();
				assert_eq!(
					reopened
						.assistant_export_turn("assistant".into(), "personal".into())
						.await
						.unwrap(),
					Some(("native".into(), turn.into()))
				);
				let key = json!(["export", "personal", "native", turn]).to_string();
				assert!(
					reopened
						.reserve_assistant_memory(
							"assistant".into(),
							key.clone(),
							json!({
								"operation":"export","memoryId":"personal","threadId":"native","turnId":turn
							})
						)
						.await
						.unwrap()
				);
				if let Some(state) = acknowledgment {
					reopened
						.acknowledge_assistant_memory("assistant".into(), key, state.into())
						.await
						.unwrap();
				}
			}
			assert!(
				store
					.assistant_export_turn("assistant".into(), "personal".into())
					.await
					.unwrap()
					.is_none()
			);
		}
	}

	#[tokio::test]
	async fn automatic_memory_does_not_relearn_a_forget_turn_or_duplicate_explicit_remember() {
		for operation in ["remember", "forget"] {
			let (_dir, store, event) = setup().await;
			store
				.apply_assistant_command(
					"assistant".into(),
					"automatic".into(),
					AssistantCommand::Configure {
						settings: AssistantSettings {
							ragflow: Some(decodex_core::assistant::RagflowConnection {
								url: "https://ragflow.example.test/".into(),
								credential_id: "a".repeat(64),
								user_id: "user".into(),
								dataset_ids: vec![],
								memory_id: Some("personal".into()),
								ingest_chats: true,
							}),
							..Default::default()
						},
					},
					true,
				)
				.await
				.unwrap();
			store.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id:"completed-personal-dialogue".into(),work_item_id:"assistant".into(),event_kind:"agent_turn_completed".into(),
				payload:json!({"terminal":{"threadId":"old","turn":{"id":"decision-turn","status":"completed"}}}).to_string()
			}).await.unwrap();
			assert!(
				store
					.assistant_export_turn("assistant".into(), "personal".into())
					.await
					.unwrap()
					.is_some()
			);
			store
				.reserve_assistant_memory(
					"assistant".into(),
					operation.into(),
					json!({"operation":operation,"memoryId":"personal","authorityEventId":event}),
				)
				.await
				.unwrap();
			assert!(
				store
					.assistant_export_turn("assistant".into(), "personal".into())
					.await
					.unwrap()
					.is_none()
			);
		}
	}

	#[tokio::test]
	async fn reset_memory_clears_old_health_and_ignores_a_late_import_result() {
		let (_dir, store, _) = setup().await;
		let connection = decodex_core::assistant::RagflowConnection {
			url: "https://ragflow.example.test/".into(),
			credential_id: "a".repeat(64),
			user_id: "user".into(),
			dataset_ids: vec!["retained-knowledge".into()],
			memory_id: Some("old-memory".into()),
			ingest_chats: true,
		};
		let mut settings =
			AssistantSettings { ragflow: Some(connection.clone()), ..Default::default() };
		store
			.apply_assistant_command(
				"assistant".into(),
				"before-reset".into(),
				AssistantCommand::Configure { settings: settings.clone() },
				true,
			)
			.await
			.unwrap();
		store
			.assistant_memory_health(
				"assistant".into(),
				connection.clone(),
				"Old import failed".into(),
			)
			.await
			.unwrap();
		assert_eq!(
			store.assistant_state().await.unwrap().memory_status.as_deref(),
			Some("Old import failed")
		);

		let reset = settings.ragflow.as_mut().unwrap();
		reset.memory_id = Some("new-memory".into());
		reset.ingest_chats = false;
		store
			.apply_assistant_command(
				"assistant".into(),
				"after-reset".into(),
				AssistantCommand::Configure { settings: settings.clone() },
				true,
			)
			.await
			.unwrap();
		assert!(store.assistant_state().await.unwrap().memory_status.is_none());
		store
			.assistant_memory_health(
				"assistant".into(),
				connection,
				"Late old import result".into(),
			)
			.await
			.unwrap();
		let observed = store.assistant_state().await.unwrap();
		assert!(observed.memory_status.is_none());
		assert_eq!(observed.settings, settings);
	}

	#[tokio::test]
	async fn assistant_memory_unknown_and_queued_receipts_survive_reopen_without_replay() {
		let (dir, store, _) = setup().await;
		let request = json!({"operation":"export","memoryId":"dedicated","turnId":"turn"});
		assert!(
			store
				.reserve_assistant_memory("assistant".into(), "exchange".into(), request.clone())
				.await
				.unwrap()
		);
		drop(store);
		let store = SqliteStore::open_test(&dir.path().join("assistant.sqlite3")).unwrap();
		assert!(
			!store
				.reserve_assistant_memory("assistant".into(), "exchange".into(), request.clone())
				.await
				.unwrap()
		);
		assert_eq!(
			store.assistant_memory_receipts("assistant".into()).await.unwrap()[0]["state"],
			"unknown"
		);
		store
			.acknowledge_assistant_memory("assistant".into(), "exchange".into(), "queued".into())
			.await
			.unwrap();
		assert_eq!(
			store.assistant_memory_receipts("assistant".into()).await.unwrap()[0]["state"],
			"queued"
		);
		assert!(
			!store
				.reserve_assistant_memory("assistant".into(), "exchange".into(), request)
				.await
				.unwrap()
		);
	}
}
