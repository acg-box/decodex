//! Recover only exact closing refusals on the retained connection.
use super::{ChiefCoordinator, ChiefError, ChiefWorkItem, ClientError, Value, json};

pub(super) struct ClosingResume {
	pub(super) thread: String,
	pub(super) turn: String,
	pub(super) revision: u64,
	pub(super) next: tokio::time::Instant,
	pub(super) attempts: u8,
}

impl ChiefCoordinator {
	pub(super) fn observe_unloaded_thread(&mut self, method: &str, thread: &str) {
		self.loaded_threads.remove(thread);
		if method != "thread/closed" {
			self.closing_resumes.retain(|_, pending| pending.thread != thread);
		}
	}

	pub(super) async fn recover_closing_threads(&mut self) -> Result<(), ChiefError> {
		if self.dispatch_paused {
			return Ok(());
		}
		let now = tokio::time::Instant::now();
		let Some(id) = self
			.closing_resumes
			.iter()
			.filter(|(_, pending)| pending.next <= now)
			.min_by_key(|(_, pending)| pending.next)
			.map(|(id, _)| id.clone())
		else {
			return Ok(());
		};
		let Some(pending) = self.closing_resumes.remove(&id) else { return Ok(()) };
		let item = self.store.get_chief_work_item(id).await?;
		if item.dispatch_state != decodex_database::ChiefDispatchState::Unknown
			|| item.codex_thread_id.as_deref() != Some(&pending.thread)
			|| item.active_turn_id.as_deref() != Some(&pending.turn)
			|| self.client.history_revision() != pending.revision
		{
			return Ok(());
		}
		self.recover_persisted_work(item, Some(pending.revision), pending.attempts).await
	}

	pub(super) async fn recover_persisted_work(
		&mut self,
		item: ChiefWorkItem,
		history_revision: Option<u64>,
		attempts: u8,
	) -> Result<(), ChiefError> {
		let (Some(thread), Some(turn)) =
			(item.codex_thread_id.as_ref(), item.active_turn_id.as_ref())
		else {
			return Ok(());
		};
		let params = Self::resume_params(thread);
		let revision = history_revision.unwrap_or_else(|| self.client.history_revision());
		let Some(guard) = self.client.history_guard(revision) else { return Ok(()) };
		let result = self.client.request_with_history("thread/resume", params, guard).await;
		let resumed = match result {
			Ok(resumed) => resumed,
			Err(ClientError::Remote(error))
				if error.code == -32600
					&& error.message.starts_with(&format!("thread {thread} is closing;")) =>
			{
				self.closing_resumes.insert(
					item.id.clone(),
					ClosingResume {
						thread: thread.clone(),
						turn: turn.clone(),
						revision,
						next: tokio::time::Instant::now()
							+ std::time::Duration::from_secs(match attempts {
								0 => 1,
								1 => 2,
								2 => 4,
								3 => 8,
								_ => 60,
							}),
						attempts: attempts.saturating_add(1),
					},
				);
				return Ok(());
			},
			Err(_) => return Ok(()),
		};
		if !Self::hydrated_thread_matches(&resumed, thread) {
			return Ok(());
		}
		self.expect_usage_replay(thread, &resumed);
		self.loaded_threads.insert(thread.clone());
		let Ok(history) = self.client.thread_read_turn(thread, turn).await else {
			return Ok(());
		};
		if history.pointer("/thread/id").and_then(Value::as_str) != Some(thread)
			|| self.client.history_revision() != revision
		{
			return Ok(());
		}
		let current = self.store.get_chief_work_item(item.id.clone()).await?;
		if current.dispatch_state != decodex_database::ChiefDispatchState::Unknown
			|| current.codex_thread_id != item.codex_thread_id
			|| current.active_turn_id != item.active_turn_id
		{
			return Ok(());
		}
		let Some(exact_turn) = history
			.pointer("/thread/turns")
			.and_then(Value::as_array)
			.and_then(|turns| turns.iter().find(|entry| entry["id"].as_str() == Some(turn)))
			.cloned()
		else {
			return Ok(());
		};
		match exact_turn["status"].as_str() {
			Some("completed" | "failed" | "interrupted") => {
				self.record_terminal(
					json!({"threadId":thread,"turn":exact_turn}),
					Ok(history),
					false,
				)
				.await?;
			},
			Some("inProgress")
				if history.pointer("/thread/status/type").and_then(Value::as_str)
					== Some("active") =>
			{
				self.store.reconcile_chief_dispatch(item.id, turn.clone()).await?;
			},
			_ => {},
		}
		Ok(())
	}
}
