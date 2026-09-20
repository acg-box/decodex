//! Atomic replacement of the native async-question projection after complete history reads.
use crate::{
	SqliteStore, StoreError, chief_questions::ChiefAsyncQuestion, error::sqlite_error, unix_micros,
};
use rusqlite::{OptionalExtension as _, params};

impl SqliteStore {
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
			for table in ["chief_async_questions", "chief_async_answers"] {
				tx.execute(&format!("DELETE FROM {table} WHERE work_id=?1 AND thread_id=?2"),params![work,thread]).map_err(sqlite_error)?;
			}
			let now=unix_micros()?;
			for question in questions {
				tx.execute("INSERT INTO chief_async_questions VALUES(?1,?2,?3,?4,?5,?6,?7)",params![work,thread,question.turn_id,question.item_id,question.question_id,question.question_json,now]).map_err(sqlite_error)?;
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
