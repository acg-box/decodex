//! Review-bound native history mutation. Persist before one write; recover by reading only.
use super::{ChiefCoordinator, ChiefError, ClientError, HistoryGuard, json};
use decodex_database::{ChiefPromptEditAttempt, ChiefPromptEditReceipt};
use sha2::{Digest as _, Sha256};
use std::time::Duration;

/// Opaque service review. The UI must show the canonical input and excluded history boundary.
/// Keep this object in the owning service; never reconstruct its guard from a client payload.
pub struct PromptEditReview {
	attempt: ChiefPromptEditAttempt,
	guard: HistoryGuard,
}
impl PromptEditReview {
	/// Native input and boundary for presentation, not permission to construct another review.
	pub fn evidence(&self) -> &ChiefPromptEditAttempt {
		&self.attempt
	}
}

impl ChiefCoordinator {
	/// Read a non-waking review for exact visible input. The caller owns visible-selection and
	/// attachment-restoration validation; this method also excludes internal voice handoffs.
	pub async fn prepare_prompt_edit(
		&self,
		work: &str,
		thread: &str,
		turn: &str,
		item: &str,
		review_id: &str,
	) -> Result<Option<PromptEditReview>, ChiefError> {
		if review_id.is_empty() || review_id.len() > 512 {
			return Err(rejected());
		}
		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());
		if !self.store.chief_thread_is_owned(work.into(), thread.into(), generation.clone()).await?
		{
			return Ok(None);
		}
		let Some(candidate) = self.client.prompt_edit_candidate(thread, turn, item).await? else {
			return Ok(None);
		};
		if super::voice_handoff(&json!({"type":"userMessage","content":candidate.content})) {
			return Ok(None);
		}
		let mut attempt = ChiefPromptEditAttempt {
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
		attempt.review_token = Sha256::digest(json!(attempt).to_string().as_bytes())
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect();
		if !candidate.guard.is_live()
			|| !self.store.chief_thread_is_owned(work.into(), thread.into(), generation).await?
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
	) -> Result<ChiefPromptEditReceipt, ChiefError> {
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
		let id = self.store.reserve_chief_prompt_edit(a.clone()).await?.ok_or_else(rejected)?;
		let response = tokio::time::timeout(
			Duration::from_secs(60),
			self.client.request_with_history(
				"thread/revert",
				json!({"threadId":a.thread,"beforeTurnId":a.before_turn_id}),
				current.guard,
			),
		)
		.await;
		if matches!(
			response,
			Ok(Err(ClientError::StaleHistory
				| ClientError::RequestTooLarge
				| ClientError::RequestQueueFull))
		) || matches!(&response,Ok(Err(ClientError::Remote(error))) if (-32602..=-32600).contains(&error.code))
		{
			self.store.reject_chief_prompt_edit_without_mutation(id, a.clone()).await?;
		} else {
			// An acknowledgement, timeout, disconnect or internal native error all require
			// readback. Failure to read is still a durable reservation, never a reason to submit
			// again.
			let _ = self.recover_prompt_edit(&a.work, &a.thread).await;
		}
		self.store.chief_prompt_edit_receipt(a.work, a.thread).await?.ok_or_else(rejected)
	}

	/// Reconnect/restart recovery performs no native mutation and does not wake a task.
	pub async fn recover_prompt_edit(
		&mut self,
		work: &str,
		thread: &str,
	) -> Result<Option<ChiefPromptEditReceipt>, ChiefError> {
		let generation = self.native_generation.as_ref().map(|g| g.as_str().to_owned());
		if !self.store.chief_thread_is_owned(work.into(), thread.into(), generation.clone()).await?
		{
			return Err(rejected());
		}
		let Some(receipt) =
			self.store.chief_prompt_edit_receipt(work.into(), thread.into()).await?
		else {
			return Ok(None);
		};
		if !matches!(receipt.state.as_str(), "reserved" | "applied") {
			return Ok(Some(receipt));
		}
		let (turns, guard) =
			tokio::time::timeout(Duration::from_secs(60), self.read_edit_history(thread))
				.await
				.map_err(|_| ChiefError::UnknownDispatch)??;
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
			self.store.observe_chief_prompt_edit(receipt.id, generation, turns).await?;
		}
		let current = self.store.chief_prompt_edit_receipt(work.into(), thread.into()).await?;
		if current.as_ref().is_some_and(|r| r.state == "applied") {
			self.observe_notification("thread/reverted", &json!({"threadId":thread})).await?;
			self.recover_async_questions().await?;
			if self.store.chief_async_questions_recovering(work.into()).await? || !guard.is_live() {
				return Err(ChiefError::UnknownDispatch);
			}
		}
		Ok(current)
	}

	async fn read_edit_history(
		&self,
		thread: &str,
	) -> Result<(Vec<String>, HistoryGuard), ChiefError> {
		let guard = self.client.thread_settings_guard(thread).ok_or_else(rejected)?;
		let metadata = self.client.thread_read(json!({"threadId":thread})).await?;
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
fn rejected() -> ChiefError {
	ChiefError::Rejected(
		"The prompt edit source changed or is unavailable. Refresh its history.".into(),
	)
}
