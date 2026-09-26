//! Read native widget documents only while the service source remains current.
use crate::chief_usage_estimate::{Source, SourceKey};
use decodex_codex::app_server_client::ClientError;
use decodex_protocol::{
	CHIEF_APP_UI_CHUNK_BYTES, ChiefAppUiRequest, ChiefAppUiResult as Result, EntityId,
	MAX_CHIEF_APP_UI_BYTES,
};
use serde_json::json;
use sha2::{Digest, Sha256};

pub(crate) async fn read<F, Fut>(source: F, request: &ChiefAppUiRequest) -> Result
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	if request.offset as usize >= MAX_CHIEF_APP_UI_BYTES
		|| (request.offset > 0 && request.fingerprint.is_none())
	{
		return Result::Unavailable;
	}
	let Some(before) = source().await else { return Result::Unavailable };
	if before.key.work != request.work_id.as_str()
		|| before.key.thread != request.thread_id.as_str()
	{
		return Result::Unavailable;
	}
	let result = tokio::time::timeout(
		std::time::Duration::from_secs(25),
		before.client.mcp_app_for_item(
			request.thread_id.as_str(),
			request.turn_id.as_str(),
			request.item_id.as_str(),
		),
	)
	.await;
	if source().await.is_none_or(|after| after.key != before.key) {
		return Result::Unavailable;
	}
	match result {
		Ok(Ok(Some(document))) if document.guard.is_live() => {
			let bytes =
				serde_json::to_vec(&json!({"item":document.item,"resources":document.resources}))
					.expect("JSON document serializes");
			if bytes.len() > MAX_CHIEF_APP_UI_BYTES {
				return Result::CapacityExceeded;
			}
			chunk(&before.key, request, bytes)
		},
		Ok(Ok(None)) => Result::Unsupported,
		Ok(Err(ClientError::Remote(error))) if error.code == -32601 => Result::Unsupported,
		Ok(Err(
			ClientError::CapacityExceeded
			| ClientError::FrameTooLarge
			| ClientError::RequestTooLarge
			| ClientError::RequestQueueFull,
		)) => Result::CapacityExceeded,
		_ => Result::Unavailable,
	}
}

pub(crate) fn source_fingerprint(key: &SourceKey) -> EntityId {
	let bytes = serde_json::to_vec(&json!([
		key.work,
		key.thread,
		key.account.as_str(),
		key.generation.as_str(),
		key.revision,
		key.history_revision
	]))
	.expect("source JSON serializes");
	EntityId::new(
		Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
	)
	.expect("SHA-256 is an entity ID")
}

fn chunk(key: &SourceKey, request: &ChiefAppUiRequest, bytes: Vec<u8>) -> Result {
	let mut hash = Sha256::new();
	// Structured fields bind every source transition and the exact native attachment.
	hash.update(
		serde_json::to_vec(&json!([
			key.generation.as_str(),
			key.account.as_str(),
			key.revision,
			key.history_revision,
			key.work,
			key.thread,
			request.turn_id,
			request.item_id,
		]))
		.expect("JSON values serialize"),
	);
	hash.update(&bytes);
	let digest: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
	if request.fingerprint.as_ref().is_some_and(|expected| expected.as_str() != digest) {
		return Result::Unavailable;
	}
	let start = request.offset as usize;
	if start >= bytes.len() {
		return Result::Unavailable;
	}
	let end = (start + CHIEF_APP_UI_CHUNK_BYTES).min(bytes.len());
	let (Ok(account_id), Ok(fingerprint)) =
		(EntityId::new(key.account.as_str()), EntityId::new(digest))
	else {
		return Result::Unavailable;
	};
	Result::Available {
		request: Box::new(request.clone()),
		account_id,
		fingerprint,
		source_fingerprint: source_fingerprint(key),
		total_bytes: bytes.len() as u32,
		bytes: bytes[start..end].to_vec(),
	}
}

#[cfg(test)]
#[path = "app_ui_tests.rs"]
mod tests;
