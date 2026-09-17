//! Source-bound provider observations. They do not authorize a turn or resolve work.

use super::*;
use decodex_codex::ThreadTokenUsage;
use sha2::{Digest as _, Sha256};

impl ChiefCoordinator {
	pub(super) async fn observe_notification(
		&self,
		method: &str,
		params: &Value,
	) -> Result<(), ChiefError> {
		let thread = exact(params, "/threadId")?;
		let turn = exact(params, "/turnId")?;
		let Some(work) = self.store.list_chief_work_items().await?.into_iter().find(|work| {
			work.codex_thread_id.as_deref() == Some(&thread)
				&& work.active_turn_id.as_deref() == Some(&turn)
		}) else {
			return Ok(());
		};
		let (kind, identity, payload) = match method {
			"thread/tokenUsage/updated" => {
				let Ok(usage) =
					serde_json::from_value::<ThreadTokenUsage>(params["tokenUsage"].clone())
				else {
					return Ok(());
				};
				if !usage.is_valid() {
					return Ok(());
				}
				let value = json!({"threadId":thread,"turnId":turn,"tokenUsage":usage});
				("token_usage", value.clone(), value)
			},
			"item/completed"
				if params["item"]["type"] == "agentMessage"
					&& params["item"]["delivery"] == "async" =>
			{
				let item_id = exact(params, "/item/id")?;
				let (messages, truncated) =
					super::result_messages::collect(Some(&json!({"items":[params["item"]]})));
				let Some(item) = messages.first() else {
					return Ok(());
				};
				(
					"assistant_message",
					json!([thread, turn, item_id]),
					json!({"threadId":thread,"turnId":turn,"item":item,"truncated":truncated}),
				)
			},
			"item/completed" if params["item"]["type"] == "contextCompaction" => {
				let item_id = exact(params, "/item/id")?;
				(
					"context_compacted",
					json!([thread, turn, item_id]),
					json!({"threadId":thread,"turnId":turn,"itemId":item_id}),
				)
			},
			_ => return Ok(()),
		};
		let digest: String = Sha256::digest(identity.to_string().as_bytes())
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect();
		self.store
			.record_chief_observation(EnqueueChiefEvent {
				source_event_id: format!("{kind}:{digest}"),
				work_item_id: work.id,
				event_kind: kind.into(),
				payload: payload.to_string(),
			})
			.await?;
		Ok(())
	}
}

pub(crate) fn usage_text(value: &Value) -> Option<String> {
	let usage: ThreadTokenUsage = serde_json::from_value(value.clone()).ok()?;
	if !usage.is_valid() {
		return None;
	}
	let mut text = format!(
		"Last response tokens: input {}, cached input {}, output {}, reasoning output {}.\nThread total tokens: {}.",
		usage.last.input_tokens,
		usage.last.cached_input_tokens,
		usage.last.output_tokens,
		usage.last.reasoning_output_tokens,
		usage.total.total_tokens
	);
	if usage.last.cache_write_input_tokens > 0 {
		text.push_str(&format!(
			"\nCache write input tokens: {}.",
			usage.last.cache_write_input_tokens
		));
	}
	if let Some(capacity) = usage.model_context_window {
		text.push_str(&format!("\nModel context capacity: {capacity} tokens."));
	}
	Some(text)
}
