//! Durable input transfer, separate from inference submission and its receipts.
use crate::{SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefPromptUpload {
	pub upload_id: String,
	pub work: String,
	pub thread: String,
	pub edit_receipt_id: i64,
	pub sha256: String,
	pub total_bytes: i64,
}

impl ChiefPromptUpload {
	fn validate(&self) -> Result<(), StoreError> {
		if self.upload_id.is_empty()
			|| self.upload_id.len() > 128
			|| self.upload_id.chars().any(char::is_control)
			|| self.total_bytes <= 0
			|| self.total_bytes > decodex_core::MAX_NATIVE_MESSAGE_BYTES as i64
			|| decodex_core::BlobHash::parse(&self.sha256).is_err()
		{
			return Err(StoreError::InvalidInput("invalid prompt upload"));
		}
		Ok(())
	}
}

fn received(c: &Connection, upload: &ChiefPromptUpload) -> Result<i64, StoreError> {
	let different: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM chief_prompt_input_chunks WHERE upload_id=?1 AND (work_item_id<>?2 OR thread_id<>?3 OR edit_receipt_id<>?4 OR sha256<>?5 OR total_bytes<>?6))", params![upload.upload_id,upload.work,upload.thread,upload.edit_receipt_id,upload.sha256,upload.total_bytes], |r| r.get(0)).map_err(sqlite_error)?;
	if different {
		return Err(StoreError::InvalidInput("prompt upload source changed"));
	}
	c.query_row("SELECT coalesce(sum(length(CAST(fragment AS BLOB))),0) FROM chief_prompt_input_chunks WHERE upload_id=?1", [&upload.upload_id], |r|r.get(0)).map_err(|error| sqlite_error(error).into())
}

fn assemble(c: &Connection, upload: &ChiefPromptUpload) -> Result<Vec<Value>, StoreError> {
	let mut statement = c.prepare("SELECT byte_offset,fragment FROM chief_prompt_input_chunks WHERE upload_id=?1 ORDER BY byte_offset").map_err(sqlite_error)?;
	let rows = statement
		.query_map([&upload.upload_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
		.map_err(sqlite_error)?;
	let mut content = String::new();
	for row in rows {
		let (offset, fragment) = row.map_err(sqlite_error)?;
		if offset != content.len() as i64
			|| content.len().saturating_add(fragment.len()) > upload.total_bytes as usize
		{
			return Err(StoreError::InvalidInput("prompt upload has a gap"));
		}
		content.push_str(&fragment);
	}
	if content.len() as i64 != upload.total_bytes
		|| decodex_core::BlobHash::digest(content.as_bytes()).to_hex() != upload.sha256
	{
		return Err(StoreError::InvalidInput("prompt upload is incomplete or its digest changed"));
	}
	let values: Vec<Value> = serde_json::from_str(&content)
		.map_err(|_| StoreError::InvalidInput("prompt upload is not an input array"))?;
	if values.is_empty() || serde_json::to_string(&values).ok().as_deref() != Some(&content) {
		return Err(StoreError::InvalidInput("prompt upload encoding is not canonical"));
	}
	Ok(values)
}

impl SqliteStore {
	/// Append one contiguous UTF-8 chunk. Repeating identical bytes is safe after restart.
	/// Commit the final chunk only if the complete digest and JSON encoding match.
	pub async fn append_chief_prompt_chunk(
		&self,
		upload: ChiefPromptUpload,
		offset: i64,
		fragment: String,
	) -> Result<i64, StoreError> {
		upload.validate()?;
		if offset < 0
			|| fragment.is_empty()
			|| fragment.len() > 65536
			|| offset.checked_add(fragment.len() as i64).is_none_or(|end| end > upload.total_bytes)
		{
			return Err(StoreError::InvalidInput("invalid prompt chunk bounds"));
		}
		self.run(move |c| {
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let receipt = crate::chief_prompt_edit::receipt(&tx,upload.edit_receipt_id)?.ok_or(StoreError::InvalidInput("prompt edit receipt is missing"))?;
			let bound: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2)", params![upload.work,upload.thread], |r|r.get(0)).map_err(sqlite_error)?;
			if !bound || receipt.attempt.work != upload.work || receipt.attempt.thread != upload.thread || !matches!(receipt.state.as_str(), "applied" | "draft_restored") {
				return Err(StoreError::InvalidInput("prompt upload does not belong to an applied edit"));
			}
			let count = received(&tx,&upload)?;
			if offset < count {
				let previous: Option<String> = tx.query_row("SELECT fragment FROM chief_prompt_input_chunks WHERE upload_id=?1 AND byte_offset=?2", params![upload.upload_id,offset], |r|r.get(0)).optional().map_err(sqlite_error)?;
				return if previous.as_deref() == Some(&fragment) { Ok(count) } else { Err(StoreError::InvalidInput("prompt chunk conflicts with saved bytes")) };
			}
			if offset != count { return Err(StoreError::InvalidInput("prompt chunk is not contiguous")); }
			tx.execute("INSERT INTO chief_prompt_input_chunks(upload_id,byte_offset,work_item_id,thread_id,edit_receipt_id,sha256,total_bytes,fragment) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![upload.upload_id,offset,upload.work,upload.thread,upload.edit_receipt_id,upload.sha256,upload.total_bytes,fragment]).map_err(sqlite_error)?;
			let count = offset + fragment.len() as i64;
			if count == upload.total_bytes { assemble(&tx,&upload)?; }
			tx.commit().map_err(sqlite_error)?;
			Ok(count)
		}).await
	}

	/// Report acknowledged bytes from durable storage, never from a transient buffer.
	pub async fn chief_prompt_upload_received(
		&self,
		upload: ChiefPromptUpload,
	) -> Result<i64, StoreError> {
		upload.validate()?;
		self.run(move |c| received(c, &upload)).await
	}

	/// Materialize the immutable input. This operation never queues or sends it.
	pub async fn complete_chief_prompt_upload(
		&self,
		upload: ChiefPromptUpload,
	) -> Result<crate::ChiefPromptInput, StoreError> {
		upload.validate()?;
		let source = upload.clone();
		let content = self
			.run(move |c| {
				received(c, &source)?;
				assemble(c, &source)
			})
			.await?;
		self.retain_chief_prompt_input(upload.work, upload.thread, upload.edit_receipt_id, content)
			.await
	}
}
