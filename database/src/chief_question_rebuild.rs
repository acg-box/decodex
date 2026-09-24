//! Atomic replacement of the native async-question projection after complete history reads.
use crate::{
	SqliteStore, StoreError, chief_questions::ChiefAsyncQuestion, error::sqlite_error, unix_micros,
};
use rusqlite::{OptionalExtension as _, params};

impl SqliteStore {
	/// Retire one exact async input after the transport proves rejection before any write.
	/// This must never be used for remote errors, timeouts, or uncertain transport failures.
	pub async fn reject_chief_async_before_write(
		&self,
		work: String,
		event: i64,
		previous: Option<crate::ChiefWorkItem>,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
			let tx=connection.transaction().map_err(sqlite_error)?;
			let valid: bool = if previous.is_none() {
				tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id WHERE e.id=?1 AND w.id=?2 AND e.event_kind='steer_pending' AND e.disposition IS NULL AND e.delivery_work_item_id=w.id AND e.delivered_turn_id=w.active_turn_id AND w.dispatch_state='running' AND json_extract(e.payload,'$.asyncQuestionId') IS NOT NULL)",params![event,work],|row|row.get(0)).map_err(sqlite_error)?
			} else {
				tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id WHERE e.id=?1 AND w.id=?2 AND e.event_kind='async_question_answer' AND e.disposition IS NULL AND e.delivery_work_item_id=w.id AND e.delivered_turn_id='' AND w.dispatch_state='dispatching' AND w.active_turn_id IS NULL AND (SELECT count(*) FROM chief_inbox_events c WHERE c.delivery_work_item_id=w.id AND c.delivered_turn_id='' AND c.disposition IS NULL)=1)",params![event,work],|row|row.get(0)).map_err(sqlite_error)?
			};
			if !valid { return Err(StoreError::InvalidInput("async input claim changed")); }
			let now=unix_micros()?;
			tx.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note='Native history changed; this input was rejected before transport write.',disposed_at_micros=max(created_at_micros,?2) WHERE id=?1",params![event,now]).map_err(sqlite_error)?;
			if let Some(previous) = previous {
				if previous.id != work || previous.dispatch_state != crate::ChiefDispatchState::Idle {
					return Err(StoreError::InvalidInput("invalid pre-dispatch state"));
				}
				tx.execute("UPDATE chief_work_items SET dispatch_state='idle',status=?3,next_check_at_micros=?4,updated_at_micros=max(updated_at_micros,?2) WHERE id=?1",params![work,now,previous.status.as_str(),previous.next_check_at_micros]).map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;Ok(())
		}).await
	}

	/// Hide reverted question state durably and retire only answers not yet claimed for delivery.
	pub async fn queue_chief_async_revert(&self, thread: String) -> Result<(), StoreError> {
		self.queue_chief_async_rebuild(thread, true).await
	}

	/// Require a new complete projection without changing local delivery receipts.
	pub async fn refresh_chief_async_projection(&self, thread: String) -> Result<(), StoreError> {
		self.queue_chief_async_rebuild(thread, false).await
	}

	async fn queue_chief_async_rebuild(
		&self,
		thread: String,
		retire_unsent: bool,
	) -> Result<(), StoreError> {
		if thread.is_empty() || thread.len() > 512 {
			return Err(StoreError::InvalidInput("invalid reverted thread"));
		}
		self.run(move |connection| {
			let tx = connection.transaction().map_err(sqlite_error)?;
			tx.execute("INSERT INTO chief_async_recovery(work_id,thread_id,required_item_id) SELECT id,codex_thread_id,NULL FROM chief_work_items WHERE codex_thread_id=?1 ON CONFLICT(work_id,thread_id) DO UPDATE SET required_item_id=NULL",[&thread]).map_err(sqlite_error)?;
			if retire_unsent {
			tx.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note='Unsent question answer retired because native history was reverted.',disposed_at_micros=max(created_at_micros,?2) WHERE work_item_id IN (SELECT id FROM chief_work_items WHERE codex_thread_id=?1) AND event_kind='async_question_answer' AND disposition IS NULL AND delivered_turn_id IS NULL",params![thread,unix_micros()?]).map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Replace only the exact current thread's projection and matching recovery marker.
	/// Delivery receipts remain authoritative for uncertain local sends and are never replayed.
	pub async fn replace_chief_async_projection(
		&self,
		work: String,
		thread: String,
		required_item: Option<String>,
		questions: Vec<ChiefAsyncQuestion>,
		answers: Vec<String>,
	) -> Result<bool, StoreError> {
		validate(&thread, &questions, &answers)?;
		self.run(move |connection| {
			let tx = connection.transaction().map_err(sqlite_error)?;
			let marker: Option<Option<String>> = tx.query_row("SELECT r.required_item_id FROM chief_async_recovery r JOIN chief_work_items w ON w.id=r.work_id AND w.codex_thread_id=r.thread_id WHERE r.work_id=?1 AND r.thread_id=?2",params![work,thread],|row|row.get(0)).optional().map_err(sqlite_error)?;
			if marker != Some(required_item) { return Ok(false); }
            let live = {
                let mut query = tx.prepare("SELECT question_id,turn_id,item_id,question_json FROM chief_async_questions WHERE work_id=?1 AND thread_id=?2 AND arrived_live=1").map_err(sqlite_error)?;
                query.query_map(params![work,thread], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?))).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?
            };
			let skipped = {
				let mut query = tx.prepare("SELECT q.question_id,q.turn_id,q.item_id,q.question_json,s.created_at_micros FROM chief_async_skips s JOIN chief_async_questions q USING(work_id,thread_id,question_id) WHERE s.work_id=?1 AND s.thread_id=?2").map_err(sqlite_error)?;
				query.query_map(params![work,thread], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,i64>(4)?))).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?
			};
			for table in ["chief_async_questions", "chief_async_answers"] {
				tx.execute(&format!("DELETE FROM {table} WHERE work_id=?1 AND thread_id=?2"),params![work,thread]).map_err(sqlite_error)?;
			}
			let now=unix_micros()?;
			for question in questions {
				tx.execute("INSERT INTO chief_async_questions VALUES(?1,?2,?3,?4,?5,?6,?7,0)",params![work,thread,question.turn_id,question.item_id,question.question_id,question.question_json,now]).map_err(sqlite_error)?;
			}
            // Rebuild can preserve prior live provenance only for the exact same question.
            for (question,turn,item,json) in live {
                tx.execute("UPDATE chief_async_questions SET arrived_live=1 WHERE work_id=?1 AND thread_id=?2 AND question_id=?3 AND turn_id=?4 AND item_id=?5 AND question_json=?6",params![work,thread,question,turn,item,json]).map_err(sqlite_error)?;
            }
			// Preserve dismissal only for an unchanged native question. Deleted or replaced
			// questions must not transfer local dismissal to another question.
			for (question,turn,item,json,created) in skipped {
				tx.execute("INSERT INTO chief_async_skips SELECT work_id,thread_id,question_id,?7 FROM chief_async_questions WHERE work_id=?1 AND thread_id=?2 AND question_id=?3 AND turn_id=?4 AND item_id=?5 AND question_json=?6",params![work,thread,question,turn,item,json,created]).map_err(sqlite_error)?;
			}
			for answer in answers {
				tx.execute("INSERT OR IGNORE INTO chief_async_answers VALUES(?1,?2,?3,?4)",params![work,thread,answer,now]).map_err(sqlite_error)?;
			}
			tx.execute("UPDATE chief_inbox_events SET disposition='resolved',disposition_note='Unsent question answer retired because the question is no longer pending in native history.',disposed_at_micros=max(created_at_micros,?3) WHERE work_item_id=?1 AND event_kind='async_question_answer' AND disposition IS NULL AND delivered_turn_id IS NULL AND NOT EXISTS(SELECT 1 FROM chief_async_questions q WHERE q.work_id=?1 AND q.thread_id=?2 AND q.question_id=json_extract(chief_inbox_events.payload,'$.asyncQuestionId') AND NOT EXISTS(SELECT 1 FROM chief_async_answers a WHERE a.work_id=q.work_id AND a.thread_id=q.thread_id AND a.question_id IN(q.question_id,q.item_id)))",params![work,thread,now]).map_err(sqlite_error)?;
			tx.execute("DELETE FROM chief_async_recovery WHERE work_id=?1 AND thread_id=?2",params![work,thread]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(true)
		}).await
	}
}

fn validate(
	thread: &str,
	questions: &[ChiefAsyncQuestion],
	answers: &[String],
) -> Result<(), StoreError> {
	let valid = |text: &str, maximum| !text.is_empty() && text.len() <= maximum;
	let mut ids = std::collections::BTreeSet::new();
	if !valid(thread, 512)
		|| questions.len() > 8192
		|| answers.len() > 8192
		|| questions.iter().any(|q| {
			q.thread_id != thread
				|| !valid(&q.turn_id, 512)
				|| !valid(&q.item_id, 512)
				|| !valid(&q.question_id, 4096)
				|| !ids.insert(q.question_id.as_str())
				|| !serde_json::from_str::<serde_json::Value>(&q.question_json)
					.is_ok_and(|v| v.is_object())
		}) || answers.iter().any(|a| !valid(a, 4096))
		|| questions
			.iter()
			.map(|q| {
				q.question_json.len() + q.question_id.len() + q.turn_id.len() + q.item_id.len()
			})
			.sum::<usize>()
			+ answers.iter().map(String::len).sum::<usize>()
			> 8 * 1024 * 1024
	{
		return Err(StoreError::InvalidInput("invalid native question projection"));
	}
	Ok(())
}
