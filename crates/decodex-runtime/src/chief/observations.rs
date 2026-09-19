//! Source-bound provider observations. They do not authorize a turn or resolve work.

use super::*;
use decodex_codex::ThreadTokenUsage;
use sha2::{Digest as _, Sha256};

impl ChiefCoordinator {
	/// Upgrade old projections from exact native history, including other-client replies.
	/// An incomplete read leaves the durable recovery marker for the next connection.
	pub(super) async fn recover_async_questions(&self) -> Result<(), ChiefError> {
		for (work, thread, required_item) in self.store.pending_chief_async_recovery().await? {
			let mut saw_required = required_item.is_none();
			let Ok(turns) = self.client.thread_turns_since(&thread, None).await else {
				continue;
			};
			let mut seen = std::collections::BTreeSet::new();
			let mut complete = true;
			for header in turns {
				let Some(turn) = header["id"].as_str() else {
					complete = false;
					break;
				};
				let Ok(history) = self.client.thread_read_turn(&thread, turn).await else {
					complete = false;
					break;
				};
				let Some(items) = history
					.pointer("/thread/turns")
					.and_then(Value::as_array)
					.and_then(|turns| turns.iter().find(|item| item["id"].as_str() == Some(turn)))
					.and_then(|turn| turn["items"].as_array())
				else {
					complete = false;
					break;
				};
				for item in items {
					saw_required |=
						item["id"].as_str().is_some_and(|id| required_item.as_deref() == Some(id));
					self.observe_async_question_item(&thread, turn, item).await?;
					if let Ok(questions) = decodex_protocol::project_chief_async_questions(item) {
						seen.extend(questions.into_iter().map(|question| question.id));
					}
					if is_plain_user_prompt(item) {
						// Only questions preceding this native prompt are retired. A replay
						// must never clear questions received later on this connection.
						let ids = seen.iter().cloned().collect::<Vec<_>>();
						for batch in ids.chunks(32) {
							self.store
								.resolve_chief_async_questions(thread.clone(), batch.to_vec())
								.await?;
						}
					}
				}
			}
			if complete && saw_required {
				self.store.finish_chief_async_recovery(work, thread).await?;
			}
		}
		Ok(())
	}

	pub(super) async fn observe_notification(
		&self,
		method: &str,
		params: &Value,
	) -> Result<(), ChiefError> {
		if method == "item/completed"
			&& let (Some(thread), Some(turn)) =
				(params["threadId"].as_str(), params["turnId"].as_str())
		{
			self.observe_async_question_item(thread, turn, &params["item"]).await?;
			if is_plain_user_prompt(&params["item"])
				&& let Some(id) = params["item"]["id"].as_str()
			{
				self.store.request_chief_async_recovery(thread.into(), id.into()).await?;
				self.recover_async_questions().await?;
			}
		}
		if method != "thread/tokenUsage/updated"
			&& !(method == "item/completed"
				&& (params["item"]["type"] == "contextCompaction"
					|| (params["item"]["type"] == "agentMessage"
						&& params["item"]["delivery"] == "async")))
		{
			return Ok(());
		}
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

	pub(super) async fn observe_async_question_item(
		&self,
		thread: &str,
		turn: &str,
		item: &Value,
	) -> Result<(), ChiefError> {
		if item["type"] == "agentMessage" && item["delivery"] == "async" {
			if let (Some(item_id), Ok(questions)) =
				(item["id"].as_str(), decodex_protocol::project_chief_async_questions(item))
				&& !questions.is_empty()
			{
				self.store
					.record_chief_async_questions(
						thread.into(),
						turn.into(),
						item_id.into(),
						questions
							.into_iter()
							.map(|question| {
								(
									question.id.clone(),
									serde_json::to_string(&question)
										.expect("serializable question"),
								)
							})
							.collect(),
					)
					.await?;
			}
		} else if item["type"] == "userMessage" {
			let mut content = item["content"]
				.as_array()
				.into_iter()
				.flatten()
				.filter(|part| part["type"] != "skill" && part["type"] != "mention");
			if let Some(part) = content.next()
				&& content.next().is_none()
				&& part["type"] == "text"
				&& let Some(text) = part["text"].as_str()
				&& let Some(replies) = decodex_protocol::parse_chief_async_question_replies(text)
				&& replies.len() <= 32
				&& replies.iter().all(|reply| {
					!reply.question_item_id.is_empty() && reply.question_item_id.len() <= 4096
				}) {
				self.store
					.resolve_chief_async_questions(
						thread.into(),
						replies.into_iter().map(|reply| reply.question_item_id).collect(),
					)
					.await?;
			}
		}
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

fn is_plain_user_prompt(item: &Value) -> bool {
	if item["type"] != "userMessage" {
		return false;
	}
	let Some(content) = item["content"].as_array() else {
		return false;
	};
	let parts = content
		.iter()
		.filter(|part| part["type"] != "skill" && part["type"] != "mention")
		.collect::<Vec<_>>();
	if let [part] = parts.as_slice()
		&& part["type"] == "text"
		&& part["text"].as_str().is_some_and(|text| {
			decodex_protocol::parse_chief_async_question_replies(text).is_some()
		}) {
		return false;
	}
	!parts.is_empty()
}
