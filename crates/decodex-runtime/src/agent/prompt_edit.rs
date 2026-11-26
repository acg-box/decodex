//! Review-bound native history mutation. Persist before one write; recover by reading only.
use std::time::Duration;

use sha2::{Digest as _, Sha256};
use tokio::time;

use crate::agent::{AgentCoordinator, AgentError, ClientError, HistoryGuard, Value};
use decodex_codex::app_server_client::ThreadForkBoundary;
use decodex_database::{
	AgentForkAttempt, AgentForkBoundary, AgentForkReceipt, AgentPromptEditAttempt,
	AgentPromptEditReceipt,
};

/// Opaque service review. The UI must show the canonical input and excluded history boundary.
/// Keep this object in the owning service; never reconstruct its guard from a client payload.
pub struct PromptEditReview {
	attempt: AgentPromptEditAttempt,
	guard: HistoryGuard,
}
impl PromptEditReview {
	pub(crate) fn is_live(&self) -> bool {
		self.guard.is_live()
	}

	/// Native input and boundary for presentation, not permission to construct another review.
	pub fn evidence(&self) -> &AgentPromptEditAttempt {
		&self.attempt
	}
}

impl AgentCoordinator {
	/// Create one explicit branch from a retained review, without changing the source.
	pub async fn fork_prompt_edit(
		&mut self,
		review: PromptEditReview,
		target_work: String,
		boundary: AgentForkBoundary,
	) -> Result<AgentForkReceipt, AgentError> {
		self.fork_prompt_edit_with_provider(review, target_work, boundary, None).await
	}

	pub(crate) async fn fork_prompt_edit_with_provider(
		&mut self,
		review: PromptEditReview,
		target_work: String,
		boundary: AgentForkBoundary,
		model_provider: Option<String>,
	) -> Result<AgentForkReceipt, AgentError> {
		let a = review.attempt;
		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());

		if a.generation != generation || !review.guard.is_live() {
			return Err(rejected());
		}

		let Some(current) =
			self.client.prompt_edit_candidate(&a.thread, &a.before_turn_id, &a.item_id).await?
		else {
			return Err(rejected());
		};

		if current.content != a.content || current.turn_ids != a.turn_ids || !review.guard.is_live()
		{
			return Err(rejected());
		}

		let attempt = AgentForkAttempt { source: a.into(), target_work, boundary, model_provider };
		self.fork_from_source(attempt, current.guard).await
	}

	/// Open a native side conversation at the live source head without reviewing or editing
	/// history.
	pub(crate) async fn open_side_chat(
		&mut self,
		work: String,
		thread: String,
		target_work: String,
	) -> Result<AgentForkReceipt, AgentError> {
		let token = decodex_core::BlobHash::digest(target_work.as_bytes()).to_hex();
		if let Some(saved) = self.store.agent_fork_receipt(work.clone(), token.clone()).await? {
			if saved.attempt.source.thread != thread
				|| saved.attempt.target_work != target_work
				|| saved.attempt.source.review.is_some()
			{
				return Err(rejected());
			}
			return self.recover_prompt_fork(&work, &token).await?.ok_or_else(rejected);
		}
		let guard = self.client.thread_settings_guard(&thread).ok_or_else(rejected)?;
		let source = decodex_database::AgentForkSource {
			work,
			thread,
			review_token: token,
			review: None,
			generation: self.native_generation.as_ref().map(|g| g.as_str().to_owned()),
		};
		self.fork_from_source(
			AgentForkAttempt {
				source,
				target_work,
				boundary: AgentForkBoundary::TemporarySideChat,
				model_provider: None,
			},
			guard,
		)
		.await
	}

	async fn fork_from_source(
		&mut self,
		attempt: AgentForkAttempt,
		guard: HistoryGuard,
	) -> Result<AgentForkReceipt, AgentError> {
		self.store.reserve_agent_fork(attempt.clone()).await?.ok_or_else(rejected)?;
		let native_boundary = match (attempt.boundary, attempt.source.review.as_ref()) {
			(AgentForkBoundary::TemporarySideChat, review) =>
				ThreadForkBoundary::TemporarySideChat(review.map(|r| r.before_turn_id.as_str())),
			(AgentForkBoundary::BeforeInput, Some(r)) =>
				ThreadForkBoundary::BeforeInput(&r.before_turn_id),
			(AgentForkBoundary::AfterTurn, Some(r)) =>
				ThreadForkBoundary::AfterTurn(&r.before_turn_id),
			(AgentForkBoundary::AttachedAfterTurn, Some(r)) =>
				ThreadForkBoundary::AttachedAfterTurn(&r.before_turn_id),
			(AgentForkBoundary::AttachedAfterTurn, None) => ThreadForkBoundary::AttachedCurrent,
			_ => return Err(rejected()),
		};
		let response = time::timeout(
			Duration::from_secs(60),
			self.client.fork_thread_with_provider(
				&attempt.source.thread,
				native_boundary,
				guard,
				attempt.model_provider.as_deref(),
			),
		)
		.await;

		match response {
			Ok(Ok(response)) => {
				let thread = response["thread"]["id"].as_str().ok_or_else(rejected)?.to_owned();

				self.store
					.record_agent_fork_identity(attempt.clone(), thread.clone())
					.await?
					.ok_or_else(rejected)?;
				if attempt.boundary == AgentForkBoundary::TemporarySideChat {
					self.side_chats.open(&thread);
				}
				self.loaded_threads.insert(thread);

				let _ = self
					.recover_prompt_fork(&attempt.source.work, &attempt.source.review_token)
					.await;
			},
			Ok(Err(
				ClientError::StaleHistory
				| ClientError::RequestTooLarge
				| ClientError::RequestQueueFull,
			)) => {
				self.store.reject_agent_fork(attempt.clone()).await?;
			},
			Ok(Err(ClientError::Remote(error))) if (-32_602..=-32_600).contains(&error.code) => {
				self.store.reject_agent_fork(attempt.clone()).await?;
			},
			_ => {}, // Unknown acceptance keeps the durable reservation; never submit another fork.
		}

		self.store
			.agent_fork_receipt(attempt.source.work, attempt.source.review_token)
			.await?
			.ok_or_else(rejected)
	}

	/// Finish a known fork by reading its identity and exact prefix. No resume or model turn.
	pub async fn recover_prompt_fork(
		&mut self,
		source_work: &str,
		review_token: &str,
	) -> Result<Option<AgentForkReceipt>, AgentError> {
		let Some(saved) =
			self.store.agent_fork_receipt(source_work.into(), review_token.into()).await?
		else {
			return Ok(None);
		};

		if saved.state != "acknowledged" {
			return Ok(Some(saved));
		}

		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());

		let source = &saved.attempt.source;
		let may_recover = if source.review.is_none() {
			self.store
				.can_access_account_thread(source_work.into(), source.thread.clone(), generation)
				.await?
		} else {
			self.store
				.agent_thread_is_restorable(source_work.into(), source.thread.clone(), generation)
				.await?
		};
		if !may_recover {
			return Err(rejected());
		}

		let thread = saved.target_thread.as_ref().ok_or_else(rejected)?;
		if saved.attempt.boundary == AgentForkBoundary::TemporarySideChat {
			// Temporary threads have no rollout to read back. The verified native fork
			// response bound this identity to the source on the current connection.
			if saved.attempt.source.generation
				!= self.native_generation.as_ref().map(|g| g.as_str().to_owned())
			{
				return Err(rejected());
			}
			self.seed_side_chat(thread).await?;
			return Ok(self
				.store
				.acknowledge_agent_fork(saved.attempt, thread.clone(), vec![])
				.await?);
		}
		let metadata = self.client.thread_read(serde_json::json!({"threadId":thread})).await?;

		if metadata["thread"]["id"] != thread.as_str()
			|| metadata["thread"]["forkedFromId"] != saved.attempt.source.thread
			|| saved
				.attempt
				.model_provider
				.as_ref()
				.is_some_and(|provider| metadata["thread"]["modelProvider"] != provider.as_str())
		{
			return Err(rejected());
		}
		if saved.attempt.boundary == AgentForkBoundary::AttachedAfterTurn
			&& !matches!(
				self.client
					.attached_thread_archive_state(thread, &saved.attempt.source.thread)
					.await?,
				decodex_codex::app_server_client::ThreadArchiveState::Active
					| decodex_codex::app_server_client::ThreadArchiveState::Archived
			) {
			return Err(rejected());
		}

		let (turns, guard) = self.read_edit_history(thread).await?;

		if !guard.is_live() {
			return Err(rejected());
		}

		if saved.attempt.source.review.is_none() && turns.is_empty() {
			// Seed the child only, before publishing it as a runnable Work. This boundary is
			// reference context and does not start a model turn.
			self.seed_side_chat(thread).await?;
		}

		let target_work = saved.attempt.target_work.clone();
		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());
		let result =
			self.store.acknowledge_agent_fork(saved.attempt, thread.clone(), turns).await?;
		if result.as_ref().is_some_and(|receipt| receipt.state == "forked") {
			let _ = self
				.store
				.observe_agent_title(
					target_work,
					thread.clone(),
					generation,
					decodex_database::AgentConversationTitle {
						name: metadata["thread"]["name"].as_str().map(str::to_owned),
						automatic: true,
						generated: false,
						reviewed_through: 0,
					},
					true,
				)
				.await;
		}
		Ok(result)
	}

	pub(crate) async fn close_side_chat(
		&mut self,
		work: &str,
		thread: &str,
	) -> Result<(), AgentError> {
		let Some(turn) =
			self.store.temporary_side_chat_turn(work.into(), thread.into()).await.map_err(|e| {
				match e {
					decodex_database::StoreError::InvalidInput(_) => rejected(),
					other => other.into(),
				}
			})?
		else {
			return Ok(());
		};
		if let Some(turn) = turn {
			// The turn may finish between the click and its interrupt.
			match self
				.client
				.turn_interrupt(serde_json::json!({"threadId":thread,"turnId":turn}))
				.await
			{
				Ok(_) => {},
				Err(error @ ClientError::Remote(_)) => {
					let metadata = self
						.client
						.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false}))
						.await?;
					if metadata["thread"]["id"] != thread
						|| !matches!(
							metadata["thread"]["status"]["type"].as_str(),
							Some("idle" | "notLoaded")
						) {
						return Err(error.into());
					}
				},
				Err(error) => return Err(error.into()),
			}
		}
		self.client.unsubscribe_thread(thread).await?;
		self.loaded_threads.remove(thread);
		self.store.discard_temporary_side_chat(work.into(), thread.into()).await?;
		self.side_chats.close(thread);
		Ok(())
	}

	async fn seed_side_chat(&self, thread: &str) -> Result<(), AgentError> {
		self.client.request("thread/inject_items",serde_json::json!({
                "threadId":thread,
                "items":[{"type":"message","role":"developer","content":[{"type":"input_text","text":"This is a side conversation. Use the parent history only as reference. Answer the user's new questions here. Do not continue the parent's tasks, approvals, or tool calls. Default to read-only exploration."}]},
                         {"type":"message","role":"user","content":[{"type":"input_text","text":"Side conversation boundary. Everything above is inherited context. Wait for my side question."}]}]
            })).await?;
		Ok(())
	}

	/// Reconnect an idle saved source when needed, then review its visible input without sending.
	/// The caller owns selection and attachment validation; internal voice handoffs are excluded.
	pub async fn prepare_prompt_edit(
		&mut self,
		work: &str,
		thread: &str,
		turn: &str,
		item: &str,
		review_id: &str,
	) -> Result<Option<PromptEditReview>, AgentError> {
		if review_id.is_empty() || review_id.len() > 512 {
			return Err(rejected());
		}

		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());

		if !self
			.store
			.agent_thread_is_restorable(work.into(), thread.into(), generation.clone())
			.await?
		{
			let source = self.store.get_agent_work_item(work.into()).await?;
			if generation.is_none() {
				return Ok(None);
			}
			if source.parent_goal_id.is_some()
				|| source.codex_thread_id.as_deref() != Some(thread)
				|| source.active_turn_id.is_some()
				|| source.dispatch_state != decodex_database::AgentDispatchState::Idle
			{
				return Ok(None);
			}
			self.hydrate_dispatch_thread(thread, &Value::Null).await?;
			if !self
				.store
				.agent_thread_is_restorable(work.into(), thread.into(), generation.clone())
				.await?
			{
				return Ok(None);
			}
		}

		let Some(candidate) = self.client.prompt_edit_candidate(thread, turn, item).await? else {
			return Ok(None);
		};

		if super::voice_handoff(
			&serde_json::json!({"type":"userMessage","content":candidate.content}),
		) {
			return Ok(None);
		}

		let mut attempt = AgentPromptEditAttempt {
			work: work.into(),
			thread: thread.into(),
			generation: generation.clone(),
			review_token: String::new(),
			attempt_id: review_id.into(),
			before_turn_id: turn.into(),
			item_id: item.into(),
			turn_ids: candidate.turn_ids,
			content: candidate.content,
		};

		attempt.review_token = Sha256::digest(serde_json::json!(attempt).to_string().as_bytes())
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect();

		if !candidate.guard.is_live()
			|| !self
				.store
				.agent_thread_is_restorable(work.into(), thread.into(), generation)
				.await?
		{
			return Ok(None);
		}

		Ok(Some(PromptEditReview { attempt, guard: candidate.guard }))
	}

	/// Confirm one exact service-held review. The reservation survives every post-write failure.
	/// A successful return can still be reserved/unknown; only applied proves native history.
	/// Even applied remains input-fenced until projection recovery and desktop draft handback.
	pub async fn confirm_prompt_edit(
		&mut self,
		review: PromptEditReview,
	) -> Result<AgentPromptEditReceipt, AgentError> {
		let a = review.attempt;
		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());

		if a.generation != generation || !review.guard.is_live() {
			return Err(rejected());
		}

		let Some(current) =
			self.client.prompt_edit_candidate(&a.thread, &a.before_turn_id, &a.item_id).await?
		else {
			return Err(rejected());
		};

		if !review.guard.is_live() || current.content != a.content || current.turn_ids != a.turn_ids
		{
			return Err(rejected());
		}

		let id = self.store.reserve_agent_prompt_edit(a.clone()).await?.ok_or_else(rejected)?;
		let response = time::timeout(
			Duration::from_secs(60),
			self.client.request_with_history(
				"thread/revert",
				serde_json::json!({"threadId":a.thread,"beforeTurnId":a.before_turn_id}),
				current.guard,
			),
		)
		.await;

		if matches!(
			response,
			Ok(Err(ClientError::StaleHistory
				| ClientError::RequestTooLarge
				| ClientError::RequestQueueFull))
		) || matches!(&response,Ok(Err(ClientError::Remote(error))) if (-32_602..=-32_600).contains(&error.code))
		{
			self.store.reject_agent_prompt_edit_without_mutation(id, a.clone()).await?;
		} else {
			// An acknowledgement, timeout, disconnect or internal native error all require
			// readback. Failure to read is still a durable reservation, never a reason to submit
			// again.
			let _ = self.recover_prompt_edit(&a.work, &a.thread).await;
		}

		self.store.agent_prompt_edit_receipt(a.work, a.thread).await?.ok_or_else(rejected)
	}

	/// Reconnect/restart recovery performs no native mutation and does not wake a task.
	pub async fn recover_prompt_edit(
		&mut self,
		work: &str,
		thread: &str,
	) -> Result<Option<AgentPromptEditReceipt>, AgentError> {
		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());

		if !self
			.store
			.agent_thread_is_restorable(work.into(), thread.into(), generation.clone())
			.await?
		{
			return Err(rejected());
		}

		let Some(receipt) =
			self.store.agent_prompt_edit_receipt(work.into(), thread.into()).await?
		else {
			return Ok(None);
		};

		if !matches!(receipt.state.as_str(), "reserved" | "applied") {
			return Ok(Some(receipt));
		}

		let (turns, guard) = time::timeout(Duration::from_secs(60), self.read_edit_history(thread))
			.await
			.map_err(|_| AgentError::UnknownDispatch)??;

		if !guard.is_live() {
			return Err(rejected());
		}
		if receipt.state == "applied" {
			let boundary = receipt
				.attempt
				.turn_ids
				.iter()
				.position(|id| id == &receipt.attempt.before_turn_id)
				.ok_or_else(rejected)?;

			if turns != receipt.attempt.turn_ids[..boundary] {
				return Err(rejected());
			}
		} else {
			self.store.observe_agent_prompt_edit(receipt.id, generation, turns).await?;
		}

		let current = self.store.agent_prompt_edit_receipt(work.into(), thread.into()).await?;

		if current.as_ref().is_some_and(|r| r.state == "applied") {
			self.observe_notification("thread/reverted", &serde_json::json!({"threadId":thread}))
				.await?;
			self.recover_async_questions().await?;

			if self.store.agent_async_questions_recovering(work.into()).await? || !guard.is_live() {
				return Err(AgentError::UnknownDispatch);
			}
		}

		Ok(current)
	}

	async fn read_edit_history(
		&self,
		thread: &str,
	) -> Result<(Vec<String>, HistoryGuard), AgentError> {
		let guard = self.client.thread_settings_guard(thread).ok_or_else(rejected)?;
		let metadata = self.client.thread_read(serde_json::json!({"threadId":thread})).await?;

		if metadata["thread"]["id"] != thread || metadata["thread"]["historyMode"] != "paginated" {
			return Err(rejected());
		}

		let headers = self.client.thread_turns_since(thread, None).await?;

		if headers.last().is_some_and(|t| {
			!matches!(t["status"].as_str(), Some("completed" | "failed" | "interrupted"))
		}) {
			return Err(rejected());
		}

		let turns = headers
			.iter()
			.map(|t| t["id"].as_str().map(str::to_owned).ok_or_else(rejected))
			.collect::<Result<Vec<_>, _>>()?;

		if self.client.thread_latest_turn_id(thread).await? != turns.last().cloned()
			|| !guard.is_live()
		{
			return Err(rejected());
		}

		Ok((turns, guard))
	}
}
fn rejected() -> AgentError {
	AgentError::Rejected(
		"The prompt edit source changed or is unavailable. Refresh its history.".into(),
	)
}
