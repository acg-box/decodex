//! Durable async question projection and committed reply tombstones.
use crate::{SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, params};

#[derive(Clone, Debug)]
pub struct ChiefAsyncQuestion {
	pub thread_id: String,
	pub turn_id: String,
	pub item_id: String,
	pub question_id: String,
	pub question_json: String,
}

impl SqliteStore {
	/// A transport-uncertain answer is not permission to create another attempt after restart.
	pub async fn chief_async_answer_pending(
		&self,
		work: String,
		question: String,
	) -> Result<bool, StoreError> {
		self.run(move |connection| {
            connection.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE work_item_id=?1 AND disposition IS NULL AND json_extract(payload,'$.asyncQuestionId')=?2 AND (event_kind='steer_pending' OR (event_kind='async_question_answer' AND delivered_turn_id IS NOT NULL)))",params![work,question],|row|row.get(0)).map_err(|error|sqlite_error(error).into())
        }).await
	}

	/// Reconcile changes made by other clients while this connection was absent.
	pub async fn queue_chief_async_reconnection(&self) -> Result<(), StoreError> {
		self.run(|connection| {
            connection.execute("INSERT OR IGNORE INTO chief_async_recovery(work_id,thread_id) SELECT id,codex_thread_id FROM chief_work_items WHERE codex_thread_id IS NOT NULL AND status<>'resolved'",[]).map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	/// Require native history to contain the observed prompt before retiring older cards.
	pub async fn request_chief_async_recovery(
		&self,
		thread: String,
		item: String,
	) -> Result<(), StoreError> {
		if thread.is_empty() || thread.len() > 512 || item.is_empty() || item.len() > 512 {
			return Err(StoreError::InvalidInput("invalid native prompt identity"));
		}
		self.run(move |connection| {
            connection.execute("INSERT INTO chief_async_recovery(work_id,thread_id,required_item_id) SELECT id,codex_thread_id,?2 FROM chief_work_items WHERE codex_thread_id=?1 ON CONFLICT(work_id,thread_id) DO UPDATE SET required_item_id=excluded.required_item_id",params![thread,item]).map_err(sqlite_error)?;
            Ok(())
        }).await
	}

	pub async fn chief_async_questions_recovering(&self, work: String) -> Result<bool, StoreError> {
		self.run(move |connection| {
            connection.query_row("SELECT EXISTS(SELECT 1 FROM chief_async_recovery r JOIN chief_work_items w ON w.id=r.work_id AND w.codex_thread_id=r.thread_id WHERE r.work_id=?1)",[work],|row|row.get(0)).map_err(|error|sqlite_error(error).into())
        }).await
	}

	pub async fn pending_chief_async_recovery(
		&self,
	) -> Result<Vec<(String, String, Option<String>)>, StoreError> {
		self.run(|connection| {
            let mut statement=connection.prepare("SELECT r.work_id,r.thread_id,r.required_item_id FROM chief_async_recovery r JOIN chief_work_items w ON w.id=r.work_id AND w.codex_thread_id=r.thread_id ORDER BY r.work_id").map_err(sqlite_error)?;
            let rows=statement.query_map([],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?;
            Ok(rows)
        }).await
	}

	pub async fn finish_chief_async_recovery(
		&self,
		work: String,
		thread: String,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
			connection
				.execute(
					"DELETE FROM chief_async_recovery WHERE work_id=?1 AND thread_id=?2",
					params![work, thread],
				)
				.map_err(sqlite_error)?;
			Ok(())
		})
		.await
	}

	/// Preserve exact source identity; replay cannot overwrite a question or reopen an answer.
	pub async fn record_chief_async_questions(
		&self,
		thread: String,
		turn: String,
		item: String,
		questions: Vec<(String, String)>,
	) -> Result<(), StoreError> {
		if thread.is_empty()
			|| thread.len() > 512
			|| turn.is_empty()
			|| turn.len() > 512
			|| item.is_empty()
			|| item.len() > 512
			|| questions.len() > 32
			|| questions.iter().map(|(id, json)| id.len() + json.len()).sum::<usize>() > 32768
		{
			return Err(StoreError::InvalidInput("invalid async question collection"));
		}
		for (id, json) in &questions {
			if id.is_empty()
				|| id.len() > 4096
				|| !serde_json::from_str::<serde_json::Value>(json)
					.is_ok_and(|value| value.is_object())
			{
				return Err(StoreError::InvalidInput("invalid async question"));
			}
		}
		self.run(move |connection| {
            let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
            let work:Option<String>=tx.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1",[&thread],|row|row.get(0)).optional().map_err(sqlite_error)?;
            let Some(work)=work else{return Ok(());};
            let now=unix_micros()?;
            for (id,json) in questions {
                let prior:Option<(String,String,String)>=tx.query_row("SELECT turn_id,item_id,question_json FROM chief_async_questions WHERE work_id=?1 AND thread_id=?2 AND question_id=?3",params![work,thread,id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(sqlite_error)?;
                if let Some(prior)=prior {
                    if prior!=(turn.clone(),item.clone(),json.clone()) {return Err(StoreError::IdempotencyConflict);}
                } else {
                    tx.execute("INSERT INTO chief_async_questions VALUES(?1,?2,?3,?4,?5,?6,?7)",params![work,thread,turn,item,id,json,now]).map_err(sqlite_error)?;
                }
            }
            tx.commit().map_err(sqlite_error)?; Ok(())
        }).await
	}

	/// A committed native reply can precede question history. Store its identity independently.
	pub async fn resolve_chief_async_questions(
		&self,
		thread: String,
		ids: Vec<String>,
	) -> Result<(), StoreError> {
		if thread.len() > 512
			|| ids.len() > 32
			|| ids.iter().any(|id| id.is_empty() || id.len() > 4096)
		{
			return Err(StoreError::InvalidInput("invalid async reply identities"));
		}
		self.run(move |connection| {
			let tx = connection
				.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			let work: Option<String> = tx
				.query_row(
					"SELECT id FROM chief_work_items WHERE codex_thread_id=?1",
					[&thread],
					|row| row.get(0),
				)
				.optional()
				.map_err(sqlite_error)?;
			let Some(work) = work else {
				return Ok(());
			};
			let now = unix_micros()?;
			for id in ids {
				tx.execute(
					"INSERT OR IGNORE INTO chief_async_answers VALUES(?1,?2,?3,?4)",
					params![work, thread, id, now],
				)
				.map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		})
		.await
	}

	/// Current native thread only; resolved question and legacy whole-message replies are excluded.
	pub async fn read_chief_async_questions(
		&self,
		work: String,
	) -> Result<Vec<ChiefAsyncQuestion>, StoreError> {
		self.run(move |connection| {
            let mut statement=connection.prepare("SELECT q.thread_id,q.turn_id,q.item_id,q.question_id,q.question_json FROM chief_async_questions q JOIN chief_work_items w ON w.id=q.work_id AND w.codex_thread_id=q.thread_id WHERE q.work_id=?1 AND NOT EXISTS(SELECT 1 FROM chief_async_recovery r WHERE r.work_id=q.work_id AND r.thread_id=q.thread_id) AND NOT EXISTS(SELECT 1 FROM chief_async_answers a WHERE a.work_id=q.work_id AND a.thread_id=q.thread_id AND a.question_id IN(q.question_id,q.item_id)) ORDER BY q.created_at_micros,q.rowid LIMIT 33").map_err(sqlite_error)?;
            let result=statement.query_map([work],|row|Ok(ChiefAsyncQuestion {thread_id:row.get(0)?,turn_id:row.get(1)?,item_id:row.get(2)?,question_id:row.get(3)?,question_json:row.get(4)?})).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?;
            Ok(result)
        }).await
	}
}

/// Retain tombstones so replay cannot revive questions retired by a new prompt.
pub(crate) fn retire_for_prompt(
	tx: &rusqlite::Transaction<'_>,
	work: &str,
	payload: &str,
) -> Result<(), StoreError> {
	if serde_json::from_str::<serde_json::Value>(payload)
		.is_ok_and(|value| value["asyncQuestionReply"] == true)
	{
		return Ok(());
	}
	tx.execute("INSERT OR IGNORE INTO chief_async_answers(work_id,thread_id,question_id,created_at_micros) SELECT q.work_id,q.thread_id,q.question_id,?2 FROM chief_async_questions q JOIN chief_work_items w ON w.id=q.work_id AND w.codex_thread_id=q.thread_id WHERE q.work_id=?1", params![work,unix_micros()?]).map_err(sqlite_error)?;
	Ok(())
}
