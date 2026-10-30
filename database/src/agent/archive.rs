//! Retire conversation-local pending work after native archive confirmation.
use rusqlite::TransactionBehavior;

use crate::{SqliteStore, StoreError, agent::capacity, error};

impl SqliteStore {
	/// Record explicit delete intent so a lost native reply can be reconciled safely.
	pub async fn request_agent_thread_deletion(
		&self,
		work: String,
		thread: String,
	) -> Result<(), StoreError> {
		self.run(move |c| {
			let now = crate::unix_micros()?;
			let changed = c.execute("INSERT OR IGNORE INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT json_array('thread_delete_requested',id,?2),id,'thread_delete_requested',json_object('threadId',?2),?3,'resolved','Explicit native deletion requested.',?3 FROM agent_work_items WHERE id=?1 AND codex_thread_id=?2", rusqlite::params![work,thread,now]).map_err(error::sqlite_error)?;
			if changed == 0 && !c.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id=?1 AND codex_thread_id=?2)",rusqlite::params![work,thread],|row|row.get::<_,bool>(0)).map_err(error::sqlite_error)? {
				return Err(StoreError::InvalidInput("conversation binding changed"));
			}
			Ok(())
		}).await
	}

	pub async fn agent_thread_delete_requested(
		&self,
		work: String,
		thread: String,
	) -> Result<bool, StoreError> {
		self.run(move |c| c.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE source_event_id=json_array('thread_delete_requested',?1,?2))",rusqlite::params![work,thread],|row|row.get(0)).map_err(|e|error::sqlite_error(e).into())).await
	}

	pub async fn agent_thread_is_deleted(
		&self,
		work: String,
		thread: String,
	) -> Result<bool, StoreError> {
		self.run(move |c|c.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE source_event_id=json_array('thread_deleted',?1,?2))",rusqlite::params![work,thread],|row|row.get(0)).map_err(|e|error::sqlite_error(e).into())).await
	}

	/// Retain an exact deletion fact; Work evidence is independent of chat history.
	pub async fn confirm_agent_thread_deletion(&self, thread: String) -> Result<(), StoreError> {
		self.retire_archived_agent_threads(vec![thread.clone()]).await?;
		self.run(move |c| {
			let now = crate::unix_micros()?;
			c.execute("INSERT OR IGNORE INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT json_array('thread_deleted',id,?1),id,'thread_deleted',json_object('threadId',?1),?2,'resolved','Native conversation permanently deleted.',?2 FROM agent_work_items WHERE codex_thread_id=?1",rusqlite::params![thread,now]).map_err(error::sqlite_error)?;
			Ok(())
		}).await
	}

	pub async fn deleted_agent_thread_ids(
		&self,
	) -> Result<std::collections::HashSet<String>, StoreError> {
		self.run(|c| {
			c.prepare("SELECT DISTINCT w.codex_thread_id FROM agent_work_items w WHERE w.codex_thread_id IS NOT NULL AND EXISTS(SELECT 1 FROM agent_inbox_events e WHERE e.source_event_id=json_array('thread_deleted',w.id,w.codex_thread_id))").map_err(error::sqlite_error)?.query_map([],|row|row.get(0)).map_err(error::sqlite_error)?.collect::<Result<_,_>>().map_err(|e|error::sqlite_error(e).into())
		}).await
	}

	/// End local input and attention lifecycles for confirmed archived native threads.
	/// Sent history, in-flight delivery receipts and Project/Work records remain intact.
	pub async fn retire_archived_agent_threads(
		&self,
		threads: Vec<String>,
	) -> Result<(), StoreError> {
		if threads.is_empty() {
			return Ok(());
		}

		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;
			let now = crate::unix_micros()?;

			for thread in threads {
				let owners = tx.prepare("SELECT id FROM agent_work_items WHERE codex_thread_id=?1")
					.map_err(error::sqlite_error)?
					.query_map([&thread], |row| row.get::<_, String>(0))
					.map_err(error::sqlite_error)?
					.collect::<Result<Vec<_>, _>>().map_err(error::sqlite_error)?;

				for owner in owners {
					// NULL means no native dispatch was attempted; an empty turn ID is
					// a claimed delivery whose outcome must still be reconciled.
					tx.execute("DELETE FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='user_message' AND delivered_turn_id IS NULL AND (disposition IS NULL OR disposition='user_decision')", [&owner]).map_err(error::sqlite_error)?;

					capacity::cancel_pending_with_note(&tx, &owner, "Automatic retry cancelled because the conversation was archived.")?;

					tx.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note='Pending action cancelled because the conversation was archived.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id=?1 AND disposition IS NULL AND event_kind IN ('async_question_answer','steer_pending','user_input_pending','permission_pending','server_request_pending','thread_in_use_needs_attention','wake_failed','followup_processing_failed','reconnection_needs_attention','event_processing_failed')", rusqlite::params![owner,now]).map_err(error::sqlite_error)?;
					// Reuse skipped-question identities so a history replay cannot
					// turn an archived prompt into a new question after restoration.
					tx.execute("INSERT OR IGNORE INTO agent_async_skips(work_id,thread_id,question_id,created_at_micros) SELECT work_id,thread_id,question_id,?3 FROM agent_async_questions WHERE work_id=?1 AND thread_id=?2", rusqlite::params![owner,thread,now]).map_err(error::sqlite_error)?;
					tx.execute("DELETE FROM agent_async_recovery WHERE work_id=?1 AND thread_id=?2", rusqlite::params![owner,thread]).map_err(error::sqlite_error)?;
					tx.execute("UPDATE agent_work_items SET next_check_at_micros=NULL WHERE id=?1 AND next_check_at_micros IS NOT NULL", [&owner]).map_err(error::sqlite_error)?;
				}
			}

			tx.commit().map_err(error::sqlite_error)?;

			Ok(())
		}).await
	}
}

#[cfg(test)]
mod tests {
	use crate::{SqliteStore, error};

	#[tokio::test]
	async fn archive_retires_inputs_questions_and_retries_without_erasing_sent_history() {
		let root = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&root.path().join("archive.sqlite3")).unwrap();

		store.with_connection(|c| {
			for owner in ["archived","active"] {
				c.execute("INSERT INTO agent_work_items(id,kind,title,instructions,codex_thread_id,dispatch_state,status,next_check_at_micros,created_at_micros,updated_at_micros) VALUES(?1,'goal',?1,'Keep requirements',?1,'idle','open',100,1,1)",[owner]).map_err(error::sqlite_error)?;

				for (suffix,kind,disposition,turn) in [
					("failed","user_message",Some("user_decision"),None),
					("queued","user_message",None,None),
					("sent","user_message",Some("resolved"),Some("native-turn")),
					("uncertain","user_message",None,Some("")),
					("question","user_input_pending",None,None),
					("attention","thread_in_use_needs_attention",None,None),
					("retry","capacity_retry",None,None),
					("result","agent_turn_completed",Some("resolved"),None),
				] {
					c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,?3,'{}',1,?4,?5,?6,?7,?8)",rusqlite::params![format!("{owner}-{suffix}"),owner,kind,disposition,disposition.map(|_|"Kept fact"),disposition.map(|_|1),turn.map(|_|owner),turn]).map_err(error::sqlite_error)?;
				}

				c.execute("INSERT INTO agent_capacity_retries(event_id,work_item_id,failed_turn_id,attempt,due_at_micros,state) SELECT id,?1,'failed-turn',1,100,'pending' FROM agent_inbox_events WHERE source_event_id=?2",rusqlite::params![owner,format!("{owner}-retry")]).map_err(error::sqlite_error)?;
				c.execute("INSERT INTO agent_async_questions VALUES(?1,?1,'turn','item','question','{}',1,0)",[owner]).map_err(error::sqlite_error)?;
			}

			Ok(())
		}).unwrap();

		for _ in 0..2 {
			store.retire_archived_agent_threads(vec!["archived".into()]).await.unwrap();
		}

		store.with_connection(|c| {
			let count = |sql:&str| c.query_row(sql,[],|r|r.get::<_,i64>(0)).map_err(error::sqlite_error);

			assert_eq!(count("SELECT count(*) FROM agent_inbox_events WHERE work_item_id='active'")?,8);
			assert_eq!(count("SELECT count(*) FROM agent_inbox_events WHERE source_event_id IN('archived-failed','archived-queued')")?,0);
			assert_eq!(count("SELECT count(*) FROM agent_inbox_events WHERE source_event_id IN('archived-sent','archived-uncertain','archived-result') AND payload='{}'")?,3);
			assert_eq!(count("SELECT count(*) FROM agent_inbox_events WHERE source_event_id IN('archived-question','archived-attention','archived-retry') AND disposition='resolved'")?,3);
			assert_eq!(count("SELECT count(*) FROM agent_capacity_retries WHERE work_item_id='archived' AND state='cancelled'")?,1);
			assert_eq!(count("SELECT count(*) FROM agent_async_skips WHERE work_id='archived'")?,1);
			assert_eq!(count("SELECT count(*) FROM agent_async_answers WHERE work_id='archived'")?,0);
			assert_eq!(count("SELECT count(*) FROM agent_work_items WHERE id='active' AND next_check_at_micros=100")?,1);
			assert_eq!(count("SELECT count(*) FROM agent_work_items WHERE id='archived' AND status='open' AND instructions='Keep requirements' AND next_check_at_micros IS NULL")?,1);

			Ok(())
		}).unwrap();

		assert!(store.read_agent_async_questions("archived".into()).await.unwrap().is_empty());
		assert_eq!(store.read_agent_async_questions("active".into()).await.unwrap().len(), 1);
	}
	#[tokio::test]
	async fn deletion_receipts_survive_reopen_without_hiding_rebound_or_unrelated_work() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().join("delete.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store.with_connection(|c| {
			for id in ["removed","kept"] {
				c.execute("INSERT INTO agent_work_items(id,kind,title,instructions,codex_thread_id,dispatch_state,status,created_at_micros,updated_at_micros) VALUES(?1,'goal',?1,'Keep work evidence',?1,'idle','open',1,1)",[id]).map_err(error::sqlite_error)?;
			}
			Ok(())
		}).unwrap();
		assert!(
			store.request_agent_thread_deletion("removed".into(), "kept".into()).await.is_err()
		);
		for _ in 0..2 {
			store.request_agent_thread_deletion("removed".into(), "removed".into()).await.unwrap();
			store.confirm_agent_thread_deletion("removed".into()).await.unwrap();
		}
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert_eq!(
			store.deleted_agent_thread_ids().await.unwrap(),
			std::collections::HashSet::from(["removed".into()])
		);
		assert!(store.agent_thread_is_deleted("removed".into(), "removed".into()).await.unwrap());
		assert!(!store.agent_thread_is_deleted("kept".into(), "kept".into()).await.unwrap());
		store
			.with_connection(|c| {
				assert_eq!(
					c.query_row("SELECT count(*) FROM agent_inbox_events", [], |r| r
						.get::<_, i64>(0))
						.map_err(error::sqlite_error)?,
					2
				);
				assert_eq!(
					c.query_row(
						"SELECT instructions FROM agent_work_items WHERE id='removed'",
						[],
						|r| r.get::<_, String>(0)
					)
					.map_err(error::sqlite_error)?,
					"Keep work evidence"
				);
				c.execute(
					"UPDATE agent_work_items SET codex_thread_id='replacement' WHERE id='removed'",
					[],
				)
				.map_err(error::sqlite_error)?;
				Ok(())
			})
			.unwrap();
		assert!(store.deleted_agent_thread_ids().await.unwrap().is_empty());
	}
}
