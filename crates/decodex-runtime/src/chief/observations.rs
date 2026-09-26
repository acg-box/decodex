//! Source-bound provider observations. They do not authorize a turn or resolve work.

use super::{ChiefCoordinator, ChiefError, EnqueueChiefEvent, Value, exact, json};
use decodex_codex::ThreadTokenUsage;
use sha2::{Digest as _, Sha256};

impl ChiefCoordinator {
	pub(super) async fn observe_steer_receipt(
		&self,
		thread: &str,
		turn: &str,
		item: &Value,
	) -> Result<(), ChiefError> {
		if item["type"] == "userMessage"
			&& let Some(client_id) = item["clientId"].as_str()
			&& !client_id.is_empty()
			&& client_id.len() <= 512
		{
			self.store
				.observe_chief_steer_receipt(
					thread.into(),
					turn.into(),
					client_id.into(),
					self.native_generation.as_ref().map(|id| id.as_str().to_owned()),
				)
				.await?;
		}
		Ok(())
	}

	pub(super) async fn observe_misalignment(
		&mut self,
		thread: &str,
		turn: &str,
		error: &Value,
	) -> Result<(), ChiefError> {
		if error["codexErrorInfo"] != "misalignmentPolicyViolation" {
			return Ok(());
		}
		let details = super::misalignment::details(error);
		self.store.record_chief_misalignment(thread.into(), turn.into(), details).await?;
		if self.store.list_chief_work_items().await?.into_iter().any(|work| {
			work.codex_thread_id.as_deref() == Some(thread)
				&& work.active_turn_id.as_deref() == Some(turn)
		}) {
			self.stop_voice_for_precaution(thread).await?;
		}

		Ok(())
	}

	/// Upgrade old projections from exact native history, including other-client replies.
	/// An incomplete read leaves the durable recovery marker for the next connection.
	pub(super) async fn recover_async_questions(&mut self) -> Result<(), ChiefError> {
		for (work, thread, required_item) in self.store.pending_chief_async_recovery().await? {
			let revision = self.client.question_revision();
			let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
			let mut projection = super::async_projection::Projection::default();
			let mut saw_required = required_item.is_none();
			let Ok(Ok(turns)) =
				tokio::time::timeout_at(deadline, self.client.thread_turns_since(&thread, None))
					.await
			else {
				continue;
			};
			let latest = turns.last().and_then(|turn| turn["id"].as_str()).map(str::to_owned);
			let mut complete = true;
			for header in turns {
				let Some(turn) = header["id"].as_str() else {
					complete = false;
					break;
				};
				let Ok(Ok(history)) =
					tokio::time::timeout_at(deadline, self.client.thread_read_turn(&thread, turn))
						.await
				else {
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
				if latest.as_deref() == Some(turn)
					&& let Some(native_turn) =
						history.pointer("/thread/turns").and_then(Value::as_array).and_then(
							|turns| turns.iter().find(|value| value["id"].as_str() == Some(turn)),
						) && native_turn["status"] == "failed"
					&& native_turn["error"]["codexErrorInfo"] == "misalignmentPolicyViolation"
				{
					self.store
						.restore_chief_misalignment(
							thread.clone(),
							turn.into(),
							super::misalignment::details(&native_turn["error"]),
						)
						.await?;
					self.stop_voice_for_precaution(&thread).await?;
				}
				for item in items {
					self.observe_steer_receipt(&thread, turn, item).await?;
					saw_required |=
						item["id"].as_str().is_some_and(|id| required_item.as_deref() == Some(id));
					if projection.observe(&thread, turn, item).is_err() {
						complete = false;
						break;
					}
				}
				if !complete {
					break;
				}
			}
			if complete && saw_required && self.client.question_revision() == revision {
				self.store
					.replace_chief_async_projection(
						work,
						thread.clone(),
						required_item,
						projection.questions,
						projection.answers.into_iter().collect(),
					)
					.await?;
				if self.client.question_revision() != revision {
					self.store.refresh_chief_async_projection(thread).await?;
				}
			}
		}
		Ok(())
	}

	async fn invalidate_reverted_requests(&mut self, thread: &str) -> Result<(), ChiefError> {
		self.closing_resumes.retain(|_, pending| pending.thread != thread);
		self.store
			.cancel_reverted_chief_capacity_retries(
				thread.into(),
				self.native_generation.as_ref().map(|id| id.as_str().to_owned()),
			)
			.await?;
		self.store.queue_chief_async_revert(thread.into()).await?;
		self.store
			.invalidate_chief_output(
				thread.into(),
				self.native_generation.as_ref().map(|id| id.as_str().to_owned()),
			)
			.await?;
		self.loaded_threads.remove(thread);
		self.usage_replays.remove(thread);
		for (id, event_id) in self.pending_requests.clone() {
			let event = self.store.get_chief_inbox_event(event_id).await?;
			let payload: Value = serde_json::from_str(&event.payload)
				.map_err(|_| ChiefError::Invalid("invalid persisted provider request".into()))?;
			if payload["params"]["threadId"].as_str() == Some(thread) {
				if event.disposition.is_none() {
					self.store.resolve_chief_request_event(event_id).await?;
				}
				self.pending_requests.remove(&id);
			}
		}
		Ok(())
	}

	pub(super) async fn observe_question_state_notification(
		&mut self,
		method: &str,
		params: &Value,
	) -> Result<(), ChiefError> {
		if method == "rawResponse/completed" {
			if let Some(usage) = decodex_codex::decode_response_usage(params) {
				let payload = serde_json::to_string(&usage)
					.map_err(|_| ChiefError::Invalid("invalid response usage".into()))?;
				self.store
					.record_chief_response_usage(
						self.native_generation.as_ref().map(|id| id.as_str().to_owned()),
						payload,
					)
					.await?;
			}
			return Ok(());
		}
		self.observe_notification(method, params).await?;
		if decodex_codex::app_server_client::invalidates_question_state(method, params) {
			self.handled_question_revision = self
				.handled_question_revision
				.saturating_add(1)
				.min(self.client.question_revision());
		}
		Ok(())
	}

	async fn observe_completed_input(
		&mut self,
		thread: &str,
		turn: &str,
		item: &Value,
	) -> Result<(), ChiefError> {
		self.observe_steer_receipt(thread, turn, item).await?;
		self.record_async_question_item(thread, turn, item, true).await?;
		if is_plain_user_prompt(item)
			&& let Some(id) = item["id"].as_str()
		{
			self.store.request_chief_async_recovery(thread.into(), id.into()).await?;
			self.recover_async_questions().await?;
		}
		Ok(())
	}

	pub(super) async fn observe_notification(
		&mut self,
		method: &str,
		params: &Value,
	) -> Result<(), ChiefError> {
		if method == "thread/reverted" {
			return self.invalidate_reverted_requests(&exact(params, "/threadId")?).await;
		}
		if let Some(review) = decodex_codex::guardian::decode_review(method, params) {
			self.store
				.record_chief_guardian_review(decodex_database::ChiefGuardianObservation {
					thread_id: review.thread_id,
					turn_id: review.turn_id,
					review_id: review.review_id,
					connection_id: self.connection_id.clone(),
					generation_id: self.native_generation.as_ref().map(|id| id.as_str().to_owned()),
					event_json: review.event.to_string(),
				})
				.await?;
			return Ok(());
		}
		if method == "autoApprovalReview/strictReviewRequired" {
			if let (Some(thread), Some(turn), Some(started)) = (
				params["threadId"].as_str(),
				params["turnId"].as_str(),
				params["startedAtMs"].as_i64(),
			) && started >= 0
				&& !thread.is_empty()
				&& !turn.is_empty()
				&& thread.len() <= 512
				&& turn.len() <= 512
			{
				self.store.record_chief_strict_review(thread.into(), turn.into(), started).await?;
			}
			return Ok(());
		}
		if method == "error"
			&& params["willRetry"] == false
			&& let (Some(thread), Some(turn)) =
				(params["threadId"].as_str(), params["turnId"].as_str())
		{
			self.observe_misalignment(thread, turn, &params["error"]).await?;
		}
		if method == "item/completed"
			&& let (Some(thread), Some(turn)) =
				(params["threadId"].as_str(), params["turnId"].as_str())
		{
			self.observe_completed_input(thread, turn, &params["item"]).await?;
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
		self.record_async_question_item(thread, turn, item, false).await
	}

	async fn record_async_question_item(
		&self,
		thread: &str,
		turn: &str,
		item: &Value,
		live: bool,
	) -> Result<(), ChiefError> {
		if item["type"] == "agentMessage" && item["delivery"] == "async" {
			if let (Some(item_id), Ok(questions)) =
				(item["id"].as_str(), decodex_protocol::project_chief_async_questions(item))
				&& !questions.is_empty()
			{
				let questions = questions
					.into_iter()
					.map(|question| {
						(
							question.id.clone(),
							serde_json::to_string(&question).expect("serializable question"),
						)
					})
					.collect();
				if live {
					self.store
						.record_live_chief_async_questions(
							thread.into(),
							turn.into(),
							item_id.into(),
							questions,
						)
						.await?;
				} else {
					self.store
						.record_chief_async_questions(
							thread.into(),
							turn.into(),
							item_id.into(),
							questions,
						)
						.await?;
				}
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

pub(super) fn is_plain_user_prompt(item: &Value) -> bool {
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
