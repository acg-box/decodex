//! The native thread binding identifies the conversation. Responsibility is a separate relation.
use decodex_core::work::{AgentThreadCommand, ThreadRegistration};
use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};

use super::{
	AgentDispatchState, AgentWorkItem, AgentWorkKind, bounded, is_agent, read_work, work_exists,
};
use crate::{DatabaseError, SqliteStore, StoreError, error::sqlite_error};

impl SqliteStore {
	/// Check user authority before touching the provider and recognize exact committed retries.
	pub async fn agent_thread_command_applied(
		&self,
		agent: String,
		turn: String,
		command_id: String,
		command: AgentThreadCommand,
	) -> Result<bool, StoreError> {
		let json = serde_json::to_string(&command)
			.map_err(|_| StoreError::InvalidInput("invalid conversation command"))?;
		self.run(move |c| checked_command_applied(c, &agent, &turn, &command_id, &command, &json))
			.await
	}

	/// Commit a conversation command and its receipt under the caller's current Agent turn.
	pub async fn apply_agent_thread_command(
		&self,
		agent: String,
		turn: String,
		command_id: String,
		command: AgentThreadCommand,
		admitted_title: Option<String>,
	) -> Result<(), StoreError> {
		bounded(&command_id, 512)?;
		let json = serde_json::to_string(&command)
			.map_err(|_| StoreError::InvalidInput("invalid conversation command"))?;
		self.run(move |c| {
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			if checked_command_applied(&tx,&agent,&turn,&command_id,&command,&json)? {return Ok(());}
			match command {
				AgentThreadCommand::AdoptThread { thread_id, .. } => {
					register(&tx,&thread_id,admitted_title.as_deref().ok_or(StoreError::InvalidInput("missing admitted conversation"))?,
						ThreadRegistration::Assign {agent_id:agent.clone(),expected_owner:None})?;
				}
				AgentThreadCommand::MessageThread { thread_id, text, work } => {
					bounded(&text,65_536)?;
					if let Some(scope) = &work {
						let work = crate::works::owner(&tx, &scope.work_id, &agent, &scope.requirements_id)?;
						crate::works::active(&work)?;
						if work.user_hold { return Err(DatabaseError::Conflict.into()); }
					}
					let target: String = tx.query_row("SELECT w.id FROM agent_managed_conversations m JOIN agent_work_items w ON w.id=m.conversation_id WHERE m.agent_id=?1 AND w.codex_thread_id=?2",params![agent,thread_id],|r|r.get(0)).optional().map_err(sqlite_error)?.ok_or(DatabaseError::Conflict)?;
					super::insert_event(&tx, crate::EnqueueAgentEvent{
						work_item_id:target, source_event_id:serde_json::json!(["managed_thread_message",agent,command_id]).to_string(), event_kind:"work_instruction".into(),
						payload:serde_json::json!({"source":"agent","agentId":agent,"turnId":turn,"threadId":thread_id,"commandId":command_id,"work":work,"text":text}).to_string()
					},false)?;
				}
			}
			tx.execute("INSERT INTO work_commands(id,agent_id,command_json) VALUES(?1,?2,?3)",params![command_id,agent,json]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Match a retained assignment and its acknowledged receiving turn, across later ownership
	/// changes.
	pub async fn managed_work_turn(
		&self,
		agent: String,
		thread: String,
		turn: String,
		work: String,
		revision: String,
	) -> Result<bool, StoreError> {
		self.run(move |c| {
			c.query_row("SELECT EXISTS(
				SELECT 1 FROM agent_inbox_events e JOIN work_commands cmd ON cmd.id=json_extract(e.payload,'$.commandId')
				WHERE e.event_kind='work_instruction' AND e.delivery_work_item_id=e.work_item_id
				AND e.delivered_turn_id=?3 AND e.disposition='resolved' AND cmd.agent_id=?1
				AND e.source_event_id=json_array('managed_thread_message',cmd.agent_id,cmd.id)
				AND json_extract(cmd.command_json,'$.operation')='message_thread'
				AND json_extract(cmd.command_json,'$.thread_id')=?2
				AND json_extract(cmd.command_json,'$.work.work_id')=?4
				AND json_extract(cmd.command_json,'$.work.requirements_id')=?5)",
				params![agent,thread,turn,work,revision], |row| row.get(0),
			).map_err(|error| sqlite_error(error).into())
		}).await
	}

	/// Register exactly the native thread admitted by the caller. Never starts a turn.
	pub async fn register_native_thread(
		&self,
		thread: String,
		title: String,
		relation: ThreadRegistration,
	) -> Result<AgentWorkItem, StoreError> {
		self.register_native_thread_with_role_change(thread, title, relation, None)
			.await
			.map(|(item, _)| item)
	}

	/// Return whether this transaction granted a new primary Agent role.
	pub async fn register_native_thread_with_role_change(
		&self,
		thread: String,
		title: String,
		relation: ThreadRegistration,
		root_settings: Option<String>,
	) -> Result<(AgentWorkItem, bool), StoreError> {
		self.run(move |connection| {
			let tx = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			let result = register(&tx, &thread, &title, relation)?;
			if let Some(settings) = root_settings {
				crate::agent_process::save_root_settings(&tx, &result.0.id, &settings)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(result)
		})
		.await
	}

	/// Additional conversations managed by the Agent, excluding its own primary conversation.
	pub async fn managed_conversation_ids(
		&self,
		agent_id: String,
	) -> Result<Vec<String>, StoreError> {
		self.run(move |connection| {
			connection.prepare("SELECT conversation_id FROM agent_managed_conversations WHERE agent_id=?1 ORDER BY conversation_id")
				.map_err(sqlite_error)?.query_map([agent_id], |row| row.get(0)).map_err(sqlite_error)?
				.collect::<Result<Vec<_>, _>>().map_err(|error| sqlite_error(error).into())
		}).await
	}
}

const STALE_INSTRUCTION: &str =
	"Work assignment changed before delivery. A current instruction is required.";

/// Retire positively unsent instructions when the Work changes, or before the recipient claims
/// input.
pub(crate) fn retire_stale_instructions(
	db: &rusqlite::Connection,
	work: Option<&str>,
	conversation: Option<&str>,
) -> Result<(), StoreError> {
	db.execute(
		"UPDATE agent_inbox_events AS e SET disposition='resolved',disposition_note=?3,
		 disposed_at_micros=max(created_at_micros,?4)
		 WHERE e.disposition IS NULL AND e.delivered_turn_id IS NULL AND e.event_kind='work_instruction'
		 AND (?2 IS NULL OR e.work_item_id=?2)
		 AND json_valid(e.payload) AND json_type(e.payload,'$.work')='object'
		 AND (?1 IS NULL OR json_extract(e.payload,'$.work.work_id')=?1)
		 AND NOT EXISTS (
		   SELECT 1 FROM works w
		   JOIN agent_managed_conversations m ON m.agent_id=w.owner_agent_id AND m.conversation_id=e.work_item_id
		   WHERE w.id=json_extract(e.payload,'$.work.work_id') AND w.owner_agent_id=json_extract(e.payload,'$.agentId')
		   AND w.requirements_id=json_extract(e.payload,'$.work.requirements_id') AND w.state='open' AND w.user_hold=0
		 )",
		params![work,conversation,STALE_INSTRUCTION,crate::unix_micros()?],
	).map_err(sqlite_error)?;
	Ok(())
}

pub(super) fn batch_contains_stale_instruction(
	tx: &Transaction<'_>,
	conversation: &str,
	events: &[i64],
) -> Result<bool, StoreError> {
	if events.is_empty() {
		return Ok(false);
	}
	let ids = serde_json::json!(events).to_string();
	// Ordinary user input and other event types do not need the managed Work check.
	let scoped: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE id IN (SELECT value FROM json_each(?1)) AND work_item_id=?2 AND event_kind='work_instruction' AND json_valid(payload) AND json_type(payload,'$.work')='object')", params![ids,conversation], |r| r.get(0)).map_err(sqlite_error)?;
	if !scoped {
		return Ok(false);
	}
	retire_stale_instructions(tx, None, Some(conversation))?;
	tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE id IN (SELECT value FROM json_each(?1)) AND work_item_id=?2 AND disposition_note=?3 AND delivered_turn_id IS NULL)", params![ids,conversation,STALE_INSTRUCTION], |r| r.get(0)).map_err(|e|sqlite_error(e).into())
}

fn is_private_topic(c: &rusqlite::Connection, id: &str) -> Result<bool, StoreError> {
	c.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='assistant_internal_topic')", [id], |row| row.get(0))
		.map_err(|error| sqlite_error(error).into())
}

fn checked_command_applied(
	db: &rusqlite::Connection,
	agent: &str,
	turn: &str,
	id: &str,
	command: &AgentThreadCommand,
	json: &str,
) -> Result<bool, StoreError> {
	bounded(id, 512)?;
	if crate::works::command_applied(db, agent, id, json)? {
		return Ok(true);
	}
	let live: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM agents a JOIN agent_work_items w ON w.id=a.id WHERE a.id=?1 AND w.dispatch_state='running' AND w.active_turn_id=?2)",params![agent,turn],|r|r.get(0)).map_err(sqlite_error)?;
	if !live || is_private_topic(db, agent)? {
		return Err(DatabaseError::Conflict.into());
	}
	if let AgentThreadCommand::AdoptThread { thread_id, authority_event_id } = command {
		let delivered: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE id=?1 AND work_item_id=?2 AND delivery_work_item_id=?2 AND delivered_turn_id=?3 AND event_kind='user_message' AND disposition IS NULL)",params![authority_event_id,agent,turn],|r|r.get(0)).map_err(sqlite_error)?;
		let event = super::read_event(db, *authority_event_id)?;
		let payload: serde_json::Value = serde_json::from_str(&event.payload)
			.map_err(|_| StoreError::InvalidInput("invalid user input"))?;
		if !delivered || payload["source"] != "user" || !references_thread(&payload, thread_id) {
			return Err(StoreError::InvalidInput(
				"adoption requires the exact thread in a user message delivered to this turn",
			));
		}
	}
	Ok(false)
}

fn references_thread(payload: &serde_json::Value, thread: &str) -> bool {
	if thread.is_empty() {
		return false;
	}
	let text = payload["text"].as_str().unwrap_or_default();
	let id_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
	text.match_indices(thread).any(|(start, id)| {
		!text[..start].chars().next_back().is_some_and(id_char)
			&& !text[start + id.len()..].chars().next().is_some_and(id_char)
	})
}

fn register(
	tx: &Transaction<'_>,
	thread: &str,
	title: &str,
	relation: ThreadRegistration,
) -> Result<(AgentWorkItem, bool), StoreError> {
	bounded(thread, 512)?;
	bounded(title, 1024)?;
	if super::deletion::pending(tx, thread)? || super::deletion::deleted(tx, thread)? {
		return Err(StoreError::InvalidInput(
			"This conversation is being deleted or has been deleted.",
		));
	}
	let existing: Option<String> = tx
		.query_row("SELECT id FROM agent_work_items WHERE codex_thread_id=?1", [&thread], |row| {
			row.get(0)
		})
		.optional()
		.map_err(sqlite_error)?;
	let id = match existing {
		Some(id) => id,
		None => {
			if matches!(relation, ThreadRegistration::Release { .. }) {
				return Err(DatabaseError::NotFound.into());
			}
			let reserved: bool = tx.query_row(
						"SELECT EXISTS(SELECT 1 FROM agent_thread_revisions WHERE old_thread_id=?1
						 UNION ALL SELECT 1 FROM runtime_sessions WHERE codex_thread_id=?1
						 UNION ALL SELECT 1 FROM agent_slots WHERE thread_id=?1 AND agent_key<>work_id
						 UNION ALL SELECT 1 FROM agent_closed_native_threads WHERE thread_id=?1
						 UNION ALL SELECT 1 FROM native_work_contributions WHERE child_thread_id=?1)",
						[&thread], |row| row.get(0),
					).map_err(sqlite_error)?;
			if reserved || work_exists(tx, thread)? {
				return Err(DatabaseError::Conflict.into());
			}
			tx.execute(
						"INSERT INTO agent_work_items(id,codex_thread_id,kind,title,instructions,status,created_at_micros,updated_at_micros)
						 VALUES(?1,?1,'goal',?2,'Continue this conversation.','open',?3,?3)",
						params![thread, title, crate::unix_micros()?],
					).map_err(sqlite_error)?;
			thread.to_owned()
		},
	};
	let item = read_work(tx, &id)?;
	if is_private_topic(tx, &id)? {
		return Err(StoreError::InvalidInput("An Assistant topic stays with its Assistant."));
	}
	let promoted = matches!(relation, ThreadRegistration::Promote { .. }) && !is_agent(tx, &id)?;
	if !matches!(relation, ThreadRegistration::Register) {
		update_responsibility(tx, &item, relation)?;
	}
	Ok((item, promoted))
}

fn update_responsibility(
	tx: &Transaction<'_>,
	item: &AgentWorkItem,
	relation: ThreadRegistration,
) -> Result<(), StoreError> {
	if is_agent(tx, &item.id)? {
		return if matches!(relation, ThreadRegistration::Promote { .. }) {
			Ok(())
		} else {
			Err(DatabaseError::Conflict.into())
		};
	}
	if item.parent_goal_id.is_some() || item.kind != AgentWorkKind::Goal {
		return Err(StoreError::InvalidInput("the conversation belongs to an execution tree"));
	}
	let owner: Option<String> = tx
		.query_row(
			"SELECT agent_id FROM agent_managed_conversations WHERE conversation_id=?1",
			[&item.id],
			|row| row.get(0),
		)
		.optional()
		.map_err(sqlite_error)?;
	let expected_owner = match &relation {
		ThreadRegistration::Register => return Ok(()),
		ThreadRegistration::Assign { agent_id, expected_owner } => {
			if owner.as_ref() == Some(agent_id) {
				return Ok(());
			}
			if !is_agent(tx, agent_id)? || is_private_topic(tx, agent_id)? {
				return Err(DatabaseError::NotFound.into());
			}
			expected_owner.as_ref()
		},
		ThreadRegistration::Release { expected_owner } => {
			if owner.is_none() {
				return Ok(());
			}
			Some(expected_owner)
		},
		ThreadRegistration::Promote { expected_owner } => expected_owner.as_ref(),
	};
	if owner.as_ref() != expected_owner {
		return Err(DatabaseError::Conflict.into());
	}
	if item.dispatch_state != AgentDispatchState::Idle {
		return Err(StoreError::AgentThreadInUse);
	}
	retire_stale_instructions(tx, None, Some(&item.id))?;
	let pending: bool = tx.query_row(
		"SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 AND disposition IS NULL
		 AND ((event_kind IN ('user_message','async_question_answer','work_instruction','steer_pending') AND coalesce(delivered_turn_id,'')='')
		   OR event_kind IN ('user_input_pending','permission_pending','server_request_pending','thread_in_use_needs_attention')))",
		[&item.id], |row| row.get(0),
	).map_err(sqlite_error)?;
	if pending {
		return Err(StoreError::AgentThreadInUse);
	}
	if let ThreadRegistration::Assign { agent_id, .. } = relation {
		tx.execute(
			"INSERT INTO agent_managed_conversations(conversation_id,agent_id) VALUES(?1,?2)
			 ON CONFLICT(conversation_id) DO UPDATE SET agent_id=excluded.agent_id",
			params![item.id, agent_id],
		)
		.map_err(sqlite_error)?;
	} else {
		tx.execute("DELETE FROM agent_managed_conversations WHERE conversation_id=?1", [&item.id])
			.map_err(sqlite_error)?;
		if matches!(relation, ThreadRegistration::Promote { .. }) {
			super::insert_new_agent(tx, &item.id, true, false)?;
		}
	}
	Ok(())
}
