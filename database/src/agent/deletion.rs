//! Conversation deletion removes content; only content-free completion receipts survive.
use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{SqliteStore, StoreError, error};

fn deletion_plan(
	c: &Connection,
	work: &str,
	thread: &str,
) -> Result<Option<Vec<String>>, StoreError> {
	let saved = c.query_row("SELECT payload FROM agent_inbox_events WHERE source_event_id=json_array('thread_delete_requested',?1,?2)", [work,thread], |r|r.get::<_,String>(0)).optional().map_err(error::sqlite_error)?;
	saved
		.map(|saved| {
			let value: Value = serde_json::from_str(&saved)
				.map_err(|_| StoreError::InvalidInput("invalid deletion receipt"))?;
			value.get("threads").map_or_else(
				|| Ok(vec![thread.into()]),
				|ids| {
					serde_json::from_value(ids.clone())
						.map_err(|_| StoreError::InvalidInput("invalid deletion targets"))
				},
			)
		})
		.transpose()
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_core::assistant::{AssistantCommand, AssistantSettings, RagflowConnection};

	#[tokio::test]
	async fn deleting_assistant_chat_keeps_identity_and_memory_but_starts_empty() {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("assistant.sqlite3")).unwrap();
		store.with_connection(|c| {
			c.execute_batch("INSERT INTO agent_work_items(id,kind,title,instructions,codex_thread_id,dispatch_state,status,created_at_micros,updated_at_micros) VALUES('assistant','goal','Old chat','Private chat','thread','idle','wait',1,1); INSERT INTO agents VALUES('assistant',1); INSERT INTO agent_managers VALUES('assistant');").map_err(error::sqlite_error)?;
			Ok(())
		}).unwrap();
		let settings = AssistantSettings {
			mandate: "Ongoing preferences".into(),
			renew_after_turns: 24,
			ragflow: Some(RagflowConnection {
				url: "https://memory.example.com".into(),
				credential_id: "a".repeat(64),
				user_id: "personal-user".into(),
				memory_id: Some("retained-memory".into()),
				dataset_ids: vec![],
				ingest_chats: true,
			}),
		};
		store
			.apply_assistant_command(
				"assistant".into(),
				"configure".into(),
				AssistantCommand::Configure { settings: settings.clone() },
				true,
			)
			.await
			.unwrap();
		store.request_agent_thread_deletion("assistant".into(), "thread".into()).await.unwrap();
		store.confirm_agent_thread_deletion("thread".into()).await.unwrap();
		let state = store.assistant_state().await.unwrap();
		assert_eq!(state.agent_id.as_deref(), Some("assistant"));
		assert_eq!(state.settings, settings);
		let work = store.get_agent_work_item("assistant".into()).await.unwrap();
		assert!(work.codex_thread_id.is_none() && work.active_turn_id.is_none());
		assert!(store.list_agent_wake_events("assistant".into(), 100).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn deletion_keeps_a_frozen_lineage_and_removes_all_conversation_text() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("delete.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.with_connection(|c| {
			c.execute_batch("INSERT INTO agent_work_items(id,kind,title,instructions,codex_thread_id,dispatch_state,status,created_at_micros,updated_at_micros) VALUES('chat','goal','private-delete-marker','private-delete-marker','third','idle','open',1,1),('keep','goal','Keep conversation','Keep instructions','other','idle','open',1,1); INSERT INTO agents VALUES('chat',1),('keep',0); INSERT INTO agent_thread_revisions VALUES('chat','first','second',1),('chat','second','third',2); INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES('source','chat','user_message','{\"text\":\"private-delete-marker\"}',1); INSERT INTO agent_live_output(work_id,turn_id,item_id,text) VALUES('chat','turn','item','private-delete-marker');").map_err(error::sqlite_error)?;
			Ok(())
		}).unwrap();
		store.set_assistant("chat".into(), false).await.unwrap();
		assert_eq!(
			store.request_agent_thread_deletion("chat".into(), "third".into()).await.unwrap(),
			["third", "second", "first"]
		);
		assert!(store.pending_agent_deletions().await.unwrap().contains("chat"));
		assert!(store.agent_thread_deletion_pending("first".into()).await.unwrap());
		store.confirm_agent_thread_deletion("third".into()).await.unwrap();
		assert!(store.pending_agent_deletions().await.unwrap().is_empty());
		assert_eq!(
			store.deleted_agent_thread_ids().await.unwrap(),
			["first", "second", "third"].map(String::from).into()
		);
		store
			.with_connection(|c| {
				for table in ["agent_inbox_events", "agent_live_output", "agent_thread_revisions"] {
					assert_eq!(
						c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
							.get::<_, i64>(0))
							.unwrap(),
						0
					);
				}
				assert_eq!(
					c.query_row("SELECT title FROM agent_work_items WHERE id='keep'", [], |r| {
						r.get::<_, String>(0)
					})
					.unwrap(),
					"Keep conversation"
				);
				assert_eq!(
					c.query_row("SELECT count(*) FROM agent_work_items WHERE id='chat'", [], |r| r
						.get::<_, i64>(0))
						.unwrap(),
					0
				);
				assert!(!c.prepare("PRAGMA foreign_key_check").unwrap().exists([]).unwrap());
				Ok(())
			})
			.unwrap();
		assert!(
			!std::fs::read(path)
				.unwrap()
				.windows(b"private-delete-marker".len())
				.any(|window| window == b"private-delete-marker")
		);
	}
}

impl SqliteStore {
	/// Freeze this conversation's current and previous native contexts before deletion.
	pub async fn request_agent_thread_deletion(
		&self,
		work: String,
		thread: String,
	) -> Result<Vec<String>, StoreError> {
		self.run(move |c| {
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			if let Some(plan) = deletion_plan(&tx, &work, &thread)? { return Ok(plan); }
			if tx.query_row("SELECT EXISTS(SELECT 1 FROM runtime_command_receipts WHERE operation='native_thread_deleted' AND json_extract(response_json,'$.threadId')=?1)",[&thread],|r|r.get::<_,bool>(0)).map_err(error::sqlite_error)? { return Ok(vec![thread]); }
			if !tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id=?1 AND codex_thread_id=?2)", [&work,&thread], |r| r.get::<_,bool>(0)).map_err(error::sqlite_error)? {
				return Err(StoreError::InvalidInput("conversation binding changed"));
			}
			let mut threads = vec![thread.clone()];
			let revisions = tx.prepare("SELECT new_thread_id,old_thread_id FROM agent_thread_revisions WHERE work_id=?1 ORDER BY created_at_micros DESC,rowid DESC").map_err(error::sqlite_error)?.query_map([&work], |r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(error::sqlite_error)?;
			for (new,old) in revisions { for id in [new,old] { if !threads.contains(&id) { threads.push(id); } } }
			let now = crate::unix_micros()?;
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(json_array('thread_delete_requested',?1,?2),?1,'thread_delete_requested',?3,?4,'resolved','Conversation deletion requested.',?4)",rusqlite::params![work,thread,json!({"threadId":thread,"threads":threads}).to_string(),now]).map_err(error::sqlite_error)?;
			tx.commit().map_err(error::sqlite_error)?;
			Ok(threads)
		}).await
	}

	pub async fn agent_thread_deletion_plan(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<Vec<String>>, StoreError> {
		self.run(move |c| deletion_plan(c, &work, &thread)).await
	}

	pub async fn pending_agent_deletions(&self) -> Result<HashSet<String>, StoreError> {
		self.run(|c| {
			c.prepare(
				"SELECT work_item_id FROM agent_inbox_events WHERE event_kind='thread_delete_requested'",
			)
			.map_err(error::sqlite_error)?
			.query_map([], |r| r.get(0))
			.map_err(error::sqlite_error)?
			.collect::<Result<_, _>>()
			.map_err(|e| error::sqlite_error(e).into())
		})
		.await
	}

	pub async fn agent_thread_deletion_pending(&self, thread: String) -> Result<bool, StoreError> {
		self.run(move |c| c.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events e,json_each(e.payload,'$.threads') t WHERE e.event_kind='thread_delete_requested' AND t.value=?1)",[thread],|r|r.get(0)).map_err(|e|error::sqlite_error(e).into())).await
	}

	pub async fn agent_thread_delete_requested(
		&self,
		work: String,
		thread: String,
	) -> Result<bool, StoreError> {
		Ok(self.agent_thread_deletion_plan(work, thread).await?.is_some())
	}

	pub async fn agent_thread_is_deleted(
		&self,
		_work: String,
		thread: String,
	) -> Result<bool, StoreError> {
		self.run(move |c| c.query_row("SELECT EXISTS(SELECT 1 FROM runtime_command_receipts WHERE operation='native_thread_deleted' AND json_extract(response_json,'$.threadId')=?1)",[thread],|r|r.get(0)).map_err(|e|error::sqlite_error(e).into())).await
	}

	pub async fn deleted_agent_thread_ids(&self) -> Result<HashSet<String>, StoreError> {
		self.run(|c| c.prepare("SELECT json_extract(response_json,'$.threadId') FROM runtime_command_receipts WHERE operation='native_thread_deleted'").map_err(error::sqlite_error)?.query_map([],|r|r.get(0)).map_err(error::sqlite_error)?.collect::<Result<_,_>>().map_err(|e|error::sqlite_error(e).into())).await
	}

	/// Purge local conversation content after all of its native contexts are gone.
	/// Independent Work artifacts and learned personal Memory have separate owners.
	pub async fn confirm_agent_thread_deletion(&self, thread: String) -> Result<(), StoreError> {
		self.run(move |c| {
			c.execute_batch("PRAGMA secure_delete=ON;").map_err(error::sqlite_error)?;
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			tx.execute_batch("PRAGMA defer_foreign_keys=ON;").map_err(error::sqlite_error)?;
			let now = crate::unix_micros()?;
			let owners = tx.prepare("SELECT id FROM agent_work_items WHERE codex_thread_id=?1").map_err(error::sqlite_error)?.query_map([&thread],|r|r.get::<_,String>(0)).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(error::sqlite_error)?;
			let mut threads = vec![thread];
			for owner in &owners {
				let assistant = tx.query_row("SELECT EXISTS(SELECT 1 FROM agents WHERE id=?1 AND assistant=1)",[owner],|r|r.get::<_,bool>(0)).map_err(error::sqlite_error)?;
				let settings = if assistant { Some(crate::assistant::settings(&tx,owner)?) } else {None};
				if let Some(plan) = deletion_plan(&tx,owner,&threads[0])? { threads=plan; }
				for (table,column) in [
					("agent_async_answers","work_id"),("agent_async_questions","work_id"),("agent_async_recovery","work_id"),
					("agent_capacity_retries","work_item_id"),("agent_guardian_reviews","work_id"),("agent_live_output","work_id"),
					("agent_misalignment","work_id"),("agent_process_bindings","root_id"),("agent_prompt_input_chunks","work_item_id"),
					("agent_prompt_inputs","work_item_id"),("agent_root_settings","root_id"),("agent_slots","work_id"),
					("agent_thread_revisions","work_id"),("agent_tool_versions","work_id"),("agent_usage","work_id"),
					("agent_voice_calls","work_id"),("guardian_sources","agent_id"),("project_agents","agent_id"),("agent_delegations","work_id"),
				] { tx.execute(&format!("DELETE FROM {table} WHERE {column}=?1"),[owner]).map_err(error::sqlite_error)?; }
				tx.execute("DELETE FROM agent_request_payloads WHERE event_id IN(SELECT id FROM agent_inbox_events WHERE work_item_id=?1)",[owner]).map_err(error::sqlite_error)?;
				tx.execute("DELETE FROM agent_dependencies WHERE work_item_id=?1 OR depends_on_id=?1",[owner]).map_err(error::sqlite_error)?;
				tx.execute("UPDATE projects SET lead_agent_id=NULL WHERE lead_agent_id=?1",[owner]).map_err(error::sqlite_error)?;
				tx.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note='Recipient conversation deleted.',disposed_at_micros=max(created_at_micros,?2) WHERE delivery_work_item_id=?1 AND work_item_id<>?1 AND disposition IS NULL",rusqlite::params![owner,now]).map_err(error::sqlite_error)?;
				// A Work result can reference an instruction receipt. Keep that receipt's
				// identity, but never its deleted conversation text or source label.
				let referenced_events = tx.prepare("SELECT id,created_at_micros FROM agent_inbox_events WHERE work_item_id=?1 AND (EXISTS(SELECT 1 FROM collaboration_instructions WHERE event_id=agent_inbox_events.id) OR EXISTS(SELECT 1 FROM agent_delegations WHERE instruction_event_id=agent_inbox_events.id) OR EXISTS(SELECT 1 FROM agent_prompt_inputs WHERE edit_receipt_id=agent_inbox_events.id) OR EXISTS(SELECT 1 FROM agent_prompt_input_chunks WHERE edit_receipt_id=agent_inbox_events.id) OR EXISTS(SELECT 1 FROM agent_capacity_retries WHERE event_id=agent_inbox_events.id))").map_err(error::sqlite_error)?.query_map([owner],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(error::sqlite_error)?;
				tx.execute("DELETE FROM agent_inbox_events WHERE work_item_id=?1",[owner]).map_err(error::sqlite_error)?;
				for (id,created) in referenced_events {
					tx.execute("INSERT INTO agent_inbox_events(id,source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,'deleted-event:'||?1,?2,'deleted_record','{}',?3,'resolved','Conversation deleted.',max(?3,?4))",rusqlite::params![id,owner,created,now]).map_err(error::sqlite_error)?;
				}
				tx.execute("UPDATE command_receipts SET request_json=NULL,progress_json=NULL WHERE request_json IS NOT NULL AND EXISTS(SELECT 1 FROM json_tree(request_json) WHERE type='text' AND value=?1)",[owner]).map_err(error::sqlite_error)?;
				tx.execute("DELETE FROM runtime_command_receipts WHERE entity_id=?1",[owner]).map_err(error::sqlite_error)?;
				tx.execute("UPDATE agent_work_items SET title='Deleted conversation',instructions='Deleted conversation',active_turn_id=NULL,dispatch_state='idle',status='resolved',next_check_at_micros=NULL WHERE id=?1",[owner]).map_err(error::sqlite_error)?;
				if let Some(settings) = settings {
					tx.execute("UPDATE agent_work_items SET title='Assistant',instructions='Help the user manage their Agents and teams.',codex_thread_id=NULL,status='wait' WHERE id=?1",[owner]).map_err(error::sqlite_error)?;
					crate::assistant::restore_configuration_after_deletion(&tx,owner,&threads[0],&settings)?;
					continue;
				}
				let referenced = tx.query_row("SELECT EXISTS(SELECT 1 FROM works WHERE owner_agent_id=?1 UNION ALL SELECT 1 FROM work_results WHERE agent_id=?1 UNION ALL SELECT 1 FROM work_commands WHERE agent_id=?1 UNION ALL SELECT 1 FROM collaboration_requests WHERE recipient_agent_id=?1 UNION ALL SELECT 1 FROM agent_inbox_events WHERE work_item_id=?1 OR delivery_work_item_id=?1 UNION ALL SELECT 1 FROM agent_work_items WHERE parent_goal_id=?1)",[owner],|r|r.get::<_,bool>(0)).map_err(error::sqlite_error)?;
				if !referenced {
					tx.execute("DELETE FROM agent_managers WHERE work_id=?1",[owner]).map_err(error::sqlite_error)?;
					tx.execute("DELETE FROM agents WHERE id=?1",[owner]).map_err(error::sqlite_error)?;
					tx.execute("DELETE FROM agent_work_items WHERE id=?1",[owner]).map_err(error::sqlite_error)?;
				}
			}
			for id in threads {
				tx.execute("DELETE FROM agent_voice_observed_turns WHERE thread_id=?1",[&id]).map_err(error::sqlite_error)?;
				tx.execute("DELETE FROM agent_closed_native_threads WHERE thread_id=?1",[&id]).map_err(error::sqlite_error)?;
				let hash: String = Sha256::digest(id.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
				tx.execute("INSERT OR IGNORE INTO runtime_command_receipts(idempotency_key,request_sha256,operation,entity_id,response_json,completed_at_micros) VALUES(?1,?2,'native_thread_deleted',?2,?3,?4)",rusqlite::params![format!("native-delete:{hash}"),hash,json!({"threadId":id,"workIds":owners}).to_string(),now]).map_err(error::sqlite_error)?;
			}
			tx.commit().map_err(error::sqlite_error)?;
			c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").map_err(error::sqlite_error)?;
			Ok(())
		}).await
	}
}
