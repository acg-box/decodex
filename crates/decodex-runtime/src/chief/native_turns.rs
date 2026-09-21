//! Recover the latest native-admitted turn without replaying local input.
use super::{ChiefCoordinator, ChiefError, Value, json};
use decodex_database::ChiefDispatchState;

impl ChiefCoordinator {
	pub(super) async fn recover_native_turns(&mut self) -> Result<(), ChiefError> {
		if self.dispatch_paused {
			return Ok(());
		}
		for work in self.store.list_chief_work_items().await? {
			if work.dispatch_state != ChiefDispatchState::Idle {
				continue;
			}
			let Some(thread) = work.codex_thread_id else {
				continue;
			};
			let generation = self.native_generation.as_ref().map(|id| id.as_str().to_owned());
			if !self
				.store
				.chief_thread_is_owned(work.id, thread.clone(), generation.clone())
				.await?
			{
				continue;
			}
			let revision = self.client.history_revision();
			let Ok(Some(turn)) = self.client.thread_latest_turn_id(&thread).await else { continue };
			let Ok(history) = self.client.thread_read_turn(&thread, &turn).await else { continue };
			let Some(observed) = history
				.pointer("/thread/turns")
				.and_then(Value::as_array)
				.and_then(|turns| turns.iter().find(|item| item["id"].as_str() == Some(&turn)))
				.cloned()
			else {
				continue;
			};
			if history.pointer("/thread/id").and_then(Value::as_str) != Some(&thread) {
				continue;
			}
			let terminal =
				matches!(observed["status"].as_str(), Some("completed" | "failed" | "interrupted"));
			if !terminal {
				if observed["status"] != "inProgress"
					|| history["thread"]["status"]["type"] != "active"
				{
					continue;
				}
				// Join an already active native thread, without any execution-setting override.
				let Ok(resumed) = self
					.client
					.thread_resume(
						json!({"threadId":thread,"excludeTurns":true,"experimentalRawEvents":true}),
					)
					.await
				else {
					continue;
				};
				if resumed["thread"]["id"].as_str() != Some(&thread) {
					continue;
				}
			}
			if self.client.thread_latest_turn_id(&thread).await.ok().flatten().as_deref()
				!= Some(&turn)
				|| self.client.history_guard(revision).is_none()
			{
				continue;
			}
			if !self
				.store
				.observe_chief_native_turn(
					thread.clone(),
					turn,
					generation,
					self.connection_id.clone(),
				)
				.await?
			{
				continue;
			}
			if terminal {
				self.record_terminal(
					json!({"threadId":thread,"turn":observed}),
					Ok(history),
					false,
				)
				.await?;
			} else {
				self.loaded_threads.insert(thread);
			}
		}
		Ok(())
	}
}
