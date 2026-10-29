//! Project a native anchor window while the original source remains bound.
use std::future::Future;

use decodex_protocol::{ItemContextRequest, ItemContextResult, WireText};

use crate::{agent::timeline, agent_usage_estimate::Source};

pub(crate) async fn read<F, Fut>(source: F, request: &ItemContextRequest) -> ItemContextResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return ItemContextResult::Unavailable;
	};
	let result = tokio::time::timeout(
		std::time::Duration::from_secs(20),
		before.client.thread_item_context(
			request.thread_id.as_str(),
			request.turn_id.as_str(),
			request.item_id.as_str(),
			request.cursor.as_ref().map(WireText::as_str),
			request.before,
		),
	)
	.await;
	if source().await.is_none_or(|after| after.key != before.key) {
		return ItemContextResult::Unavailable;
	}
	let value = match result {
		Ok(Ok(value)) => value,
		Ok(Err(decodex_codex::app_server_client::ClientError::Remote(error)))
			if error.code == -32_601 =>
			return ItemContextResult::Unsupported,
		_ => return ItemContextResult::Unavailable,
	};
	let Some(items) = value["data"]
		.as_array()
		.and_then(|data| data.iter().map(timeline::ordinary).collect::<Option<Vec<_>>>())
	else {
		return ItemContextResult::Unavailable;
	};
	let result = ItemContextResult::Available {
		request: request.clone(),
		items,
		previous_cursor: value["previousCursor"].as_str().and_then(|v| WireText::new(v).ok()),
		next_cursor: value["nextCursor"].as_str().and_then(|v| WireText::new(v).ok()),
	};
	if serde_json::to_vec(&result).map_or(true, |bytes| bytes.len() > 60 * 1024) {
		return ItemContextResult::Unavailable;
	}
	result
}
