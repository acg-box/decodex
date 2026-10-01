//! Restore native task configuration without applying creation defaults.
use crate::{
	agent::{AgentCoordinator, AgentError, Value},
	agent_models, agent_permissions, agent_plugins,
};

impl AgentCoordinator {
	pub(super) async fn hydrate_dispatch_thread(&mut self, thread: &str) -> Result<(), AgentError> {
		if !self.loaded_threads.contains(thread) {
			let revision = self.client.history_revision();
			let response = self
				.client
				.thread_resume(Self::resume_usage_params(thread))
				.await
				.map_err(|error| super::resume_error(error, thread))?;

			if !Self::hydrated_thread_matches(&response, thread) {
				return Err(AgentError::Invalid("resumed thread settings are invalid".into()));
			}

			let last_turn = self.expect_usage_replay(thread, &response, revision).await;

			self.store.validate_agent_usage_resume(thread.to_owned(), last_turn).await?;
			self.loaded_threads.insert(thread.to_owned());
			self.persist_task_settings(thread).await?;
		}

		Ok(())
	}

	pub(super) async fn observe_settings_notification(
		&self,
		method: &str,
		params: &Value,
	) -> Result<bool, AgentError> {
		if method != "thread/settings/updated" {
			return Ok(false);
		}

		if let Some(thread) = params["threadId"].as_str() {
			self.cancel_changed_capacity_selection(thread, &params["threadSettings"]).await?;
			self.persist_task_settings(thread).await?;
		}

		Ok(true)
	}

	pub(super) async fn persist_task_settings(&self, thread: &str) -> Result<(), AgentError> {
		let generation = self.native_generation.as_ref().map(|g| g.as_str().into());

		agent_permissions::persist_current(&self.store, &self.client, thread, generation.clone())
			.await?;
		agent_plugins::persist_current(&self.store, &self.client, thread, generation.clone())
			.await?;
		agent_models::persist_current(&self.store, &self.client, thread, generation).await?;

		Ok(())
	}

	pub(super) fn resume_usage_params(thread: &str) -> Value {
		let mut params = Self::resume_params(thread);

		params["initialTurnsPage"] =
			serde_json::json!({"limit":1,"sortDirection":"desc","itemsView":"summary"});

		params
	}

	pub(super) fn resume_params(thread: &str) -> Value {
		serde_json::json!({"threadId":thread,"excludeTurns":true,"experimentalRawEvents":true})
	}

	pub(super) fn hydrated_thread_matches(response: &Value, thread: &str) -> bool {
		let valid = |s: &str, max| {
			!s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
		};

		response["thread"]["id"].as_str() == Some(thread)
			&& response["model"].as_str().is_some_and(|s| valid(s, 256))
			&& match response.get("reasoningEffort") {
				Some(Value::Null) => true,
				Some(Value::String(s)) => valid(s, 128),
				_ => false,
			}
	}
}
