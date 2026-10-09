//! Recover the latest native-admitted turn without replaying local input.
use crate::agent::{self, AgentCoordinator, AgentError, Value};
use decodex_codex::app_server_client::NativeThreadGoalStatus;
use decodex_database::AgentDispatchState;

impl AgentCoordinator {
	pub(super) async fn recover_native_turns(&mut self) -> Result<(), AgentError> {
		if self.dispatch_paused {
			return Ok(());
		}

		for work in self.store.list_agent_work_items().await? {
			if work.dispatch_state != AgentDispatchState::Idle {
				continue;
			}

			let Some(thread) = work.codex_thread_id else {
				continue;
			};
			let generation = self.native_generation.as_ref().map(|id| id.as_str().to_owned());

			if !self
				.store
				.agent_thread_is_owned(work.id.clone(), thread.clone(), generation.clone())
				.await?
			{
				continue;
			}

			let phase = crate::startup_trace::Phase::new("recover_native_goal");
			self.resume_active_native_goal(&thread).await?;
			drop(phase);

			let revision = self.client.history_revision();
			let Ok(Some(turn)) = self.client.thread_latest_turn_id(&thread).await else { continue };
			// These exact terminal receipts already make observe_agent_native_turn reject
			// replay. Avoid fetching the full turn only to reach that same decision.
			if self
				.store
				.agent_native_terminal_recorded(work.id, thread.clone(), turn.clone())
				.await?
			{
				continue;
			}

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
				let Ok(resumed) = self.client.thread_resume(Self::resume_params(&thread)).await
				else {
					continue;
				};

				if resumed["thread"]["id"].as_str() != Some(&thread) {
					continue;
				}

				self.persist_task_settings(&thread).await?;
			}
			if self.client.thread_latest_turn_id(&thread).await.ok().flatten().as_deref()
				!= Some(&turn)
				|| self.client.history_guard(revision).is_none()
			{
				continue;
			}
			if !self
				.store
				.observe_agent_native_turn(
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
					serde_json::json!({"threadId":thread,"turn":observed}),
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

	async fn resume_active_native_goal(&mut self, thread: &str) -> Result<(), AgentError> {
		if self.loaded_threads.contains(thread) {
			return Ok(());
		}

		let Ok(Some(goal)) = self.client.thread_goal(thread).await else {
			return Ok(());
		};

		if goal.status != NativeThreadGoalStatus::Active {
			return Ok(());
		}

		// Hydrate native goal ownership; only the native scheduler admits continuation.
		// Do not reapply initial settings or submit a synthetic local input.
		if let Ok(resumed) = self.client.thread_resume(Self::resume_params(thread)).await
			&& resumed["thread"]["id"].as_str() == Some(thread)
		{
			self.persist_task_settings(thread).await?;
			self.loaded_threads.insert(thread.into());
		}

		Ok(())
	}

	pub(super) async fn observe_native_turn(&self, params: &Value) -> Result<(), AgentError> {
		if !self.dispatch_paused && params["turn"]["status"] == "inProgress" {
			self.store
				.observe_agent_native_turn(
					agent::exact(params, "/threadId")?,
					agent::exact(params, "/turn/id")?,
					self.native_generation.as_ref().map(|id| id.as_str().to_owned()),
					self.connection_id.clone(),
				)
				.await?;
		}

		Ok(())
	}
}
