//! Immutable canonical input. Admission and dispatch remain with the existing Chief owner.
use crate::{SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefPromptInput {
	pub id: i64,
	pub work: String,
	pub thread: String,
	pub edit_receipt_id: i64,
	pub sha256: String,
	pub content: Vec<Value>,
}

fn read(
	c: &Connection,
	id: i64,
	work: &str,
	thread: &str,
) -> Result<Option<ChiefPromptInput>, StoreError> {
	let row: Option<(i64, String, String)> = c.query_row(
		"SELECT edit_receipt_id,sha256,content FROM chief_prompt_inputs WHERE id=?1 AND work_item_id=?2 AND thread_id=?3",
		params![id,work,thread], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
	).optional().map_err(sqlite_error)?;
	row.map(|(edit_receipt_id, sha256, content)| {
		if decodex_core::BlobHash::digest(content.as_bytes()).to_hex() != sha256 {
			return Err(StoreError::InvalidInput("saved prompt input digest does not match"));
		}
		Ok(ChiefPromptInput {
			id,
			work: work.into(),
			thread: thread.into(),
			edit_receipt_id,
			sha256,
			content: serde_json::from_str(&content)
				.map_err(|_| StoreError::InvalidInput("saved prompt input is invalid"))?,
		})
	})
	.transpose()
}

impl SqliteStore {
	/// Find immutable data by exact source and digest without loading its full content.
	pub async fn chief_prompt_input_id(
		&self,
		work: String,
		thread: String,
		edit_receipt_id: i64,
		sha256: String,
		total_bytes: i64,
	) -> Result<Option<i64>, StoreError> {
		self.run(move |c| {
			c.query_row("SELECT id FROM chief_prompt_inputs WHERE work_item_id=?1 AND thread_id=?2 AND edit_receipt_id=?3 AND sha256=?4 AND length(CAST(content AS BLOB))=?5", params![work,thread,edit_receipt_id,sha256,total_bytes], |r|r.get(0)).optional().map_err(|error|sqlite_error(error).into())
		}).await
	}

	/// Retain a complete edited input after native history application, without queuing it.
	/// Exact content replay returns the existing identity. A changed input gets a new identity.
	pub async fn retain_chief_prompt_input(
		&self,
		work: String,
		thread: String,
		edit_receipt_id: i64,
		content: Vec<Value>,
	) -> Result<ChiefPromptInput, StoreError> {
		if content.is_empty()
			|| content
				.iter()
				.any(|part| part.get("type").and_then(Value::as_str).is_none_or(str::is_empty))
		{
			return Err(StoreError::InvalidInput("invalid canonical input"));
		}
		let encoded = serde_json::to_string(&content)
			.map_err(|_| StoreError::InvalidInput("invalid canonical input"))?;
		if encoded.len() > decodex_core::MAX_NATIVE_MESSAGE_BYTES {
			return Err(StoreError::InvalidInput("canonical input is too large"));
		}
		let hash = decodex_core::BlobHash::digest(encoded.as_bytes()).to_hex();
		self.run(move |c| {
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let receipt = crate::chief_prompt_edit::receipt(&tx, edit_receipt_id)?.ok_or(StoreError::InvalidInput("prompt edit receipt is missing"))?;
			let bound: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2)", params![work,thread], |r| r.get(0)).map_err(sqlite_error)?;
			if !bound || receipt.attempt.work != work || receipt.attempt.thread != thread || !matches!(receipt.state.as_str(), "applied" | "draft_restored") {
				return Err(StoreError::InvalidInput("prompt input does not belong to an applied edit"));
			}
			tx.execute("INSERT INTO chief_prompt_inputs(work_item_id,thread_id,edit_receipt_id,sha256,content,created_at_micros) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(edit_receipt_id,sha256) DO NOTHING", params![work,thread,edit_receipt_id,hash,encoded,unix_micros()?]).map_err(sqlite_error)?;
			let id = tx.query_row("SELECT id FROM chief_prompt_inputs WHERE edit_receipt_id=?1 AND sha256=?2", params![edit_receipt_id,hash], |r|r.get(0)).map_err(sqlite_error)?;
			let saved = read(&tx,id,&work,&thread)?.ok_or(StoreError::InvalidInput("prompt input source changed"))?;
			if saved.content != content { return Err(StoreError::InvalidInput("prompt input content changed")); }
			tx.commit().map_err(sqlite_error)?;
			Ok(saved)
		}).await
	}

	/// Load only an exact input owner. Reading is not permission to submit or replay it.
	pub async fn chief_prompt_input(
		&self,
		id: i64,
		work: String,
		thread: String,
	) -> Result<Option<ChiefPromptInput>, StoreError> {
		self.run(move |c| read(c, id, &work, &thread)).await
	}
}
