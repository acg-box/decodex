//! Restore native task configuration without applying creation defaults.
use crate::{
	agent::{AgentCoordinator, AgentError, Value},
	agent_models, agent_permissions, agent_plugins,
};

use decodex_codex::app_server_client::{AppServerClient, ClientError, NativeThreadGoalStatus};

impl AgentCoordinator {
	/// A prompt suggestion may load a dormant conversation before its first user turn.
	pub(crate) async fn prepare_sample_source(&mut self, thread: &str) -> Result<(), AgentError> {
		if sample_needs_resume(&self.client, thread).await? {
			self.hydrate_dispatch_thread(thread, &Value::Null).await?;
		}
		Ok(())
	}

	pub(crate) async fn hydrate_dispatch_thread(
		&mut self,
		thread: &str,
		turn: &Value,
	) -> Result<(), AgentError> {
		self.ensure_role_released(thread).await?;
		if !self.loaded_threads.contains(thread) {
			let mut params = self.resume_observer_params(thread, true).await?;
			for field in ["permissions", "approvalPolicy", "approvalsReviewer"] {
				if let Some(value) = turn.get(field) {
					params[field] = value.clone();
				}
			}
			self.resume_dispatch_thread(thread, params).await?;
		}
		Ok(())
	}

	/// Admit the same native session and its observed settings after every execution resume.
	pub(super) async fn resume_dispatch_thread(
		&mut self,
		thread: &str,
		params: Value,
	) -> Result<(), AgentError> {
		let expected_cwd = params.get("cwd").cloned();
		let revision = self.client.history_revision();
		let response = self
			.client
			.thread_resume(params)
			.await
			.map_err(|error| super::resume_error(error, thread))?;
		if !Self::hydrated_thread_matches(&response, thread)
			|| expected_cwd.is_some_and(|cwd| response["cwd"] != cwd)
		{
			return Err(AgentError::Invalid("resumed thread settings are invalid".into()));
		}
		self.attach_native_thread_owner(thread).await?;
		let last_turn = self.expect_usage_replay(thread, &response, revision).await;
		self.store.validate_agent_usage_resume(thread.to_owned(), last_turn).await?;
		self.loaded_threads.insert(thread.to_owned());
		self.persist_task_settings(thread).await?;
		Ok(())
	}

	pub(super) async fn attach_native_thread_owner(&self, thread: &str) -> Result<(), AgentError> {
		let Some(generation) = &self.native_generation else { return Ok(()) };
		let item = self
			.store
			.agent_work_item_for_thread(thread.to_owned())
			.await?
			.filter(|item| item.parent_goal_id.is_none());
		let Some(item) = item else { return Ok(()) };

		if !self
			.store
			.attach_independent_agent_process(item.id, thread.into(), generation.as_str().into())
			.await?
		{
			return Err(AgentError::Invalid(
				"Native conversation process ownership was not confirmed".into(),
			));
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

	pub(super) async fn resume_observer_params(
		&self,
		thread: &str,
		summary: bool,
	) -> Result<Value, AgentError> {
		let details = self.store.guardian_settings().await?.review_details;
		let mut params =
			if summary { Self::resume_usage_params(thread) } else { Self::resume_params(thread) };

		params["config"] = serde_json::json!({"features.guardian_recorded_items":details});
		if let Some(item) = self.store.agent_work_item_for_thread(thread.to_owned()).await? {
			if self.store.uses_agent_mcp_tools(item.id.clone()).await? {
				let assistant = self.store.assistant_settings(item.id.clone()).await?.is_some();
				self.managed_tools.configure(&mut params, assistant).await?;
				Self::enable_agent_features(&mut params);
			} else if item.parent_goal_id.is_none() && !self.is_manager(&item.id).await? {
				params["config"]["thread_unload_delay_secs"] = serde_json::json!(0);
			}
			if self.store.assistant_external_memory_context(item.id, thread.into()).await? {
				Self::disable_native_memory(&mut params);
			}
		}

		Ok(params)
	}

	/// Apply the service role without replacing the native model, directory, or permissions.
	pub(super) fn apply_agent_role(
		params: &mut Value,
		assistant: Option<&decodex_core::assistant::AssistantSettings>,
	) {
		let instructions = super::INSTRUCTIONS;
		let assistant_instructions = super::ASSISTANT_INSTRUCTIONS;
		params["developerInstructions"] = match assistant {
			Some(settings) => serde_json::json!(format!(
				"{instructions}\n\n{assistant_instructions}\n\nPersonal Assistant mandate (selected by the user):\n{}\nKeep stable Agent and Work identities across context renewal. Do not invent recurring duties. Use assistant_read for current teams and independent wakes.",
				settings.mandate
			)),
			None => serde_json::json!(instructions),
		};
		Self::enable_agent_features(params);
	}

	fn enable_agent_features(params: &mut Value) {
		params["config"]["tools.update_plan.enabled"] = serde_json::json!(true);
		params["config"]["features.realtime_conversation"] = serde_json::json!(true);
	}

	pub(super) fn disable_native_memory(params: &mut Value) {
		params["config"]["memories.generate_memories"] = serde_json::json!(false);
		params["config"]["memories.use_memories"] = serde_json::json!(false);
		params["config"]["features.memories"] = serde_json::json!(false);
	}

	pub(super) fn resume_usage_params(thread: &str) -> Value {
		let mut params = Self::resume_params(thread);

		params["initialTurnsPage"] =
			serde_json::json!({"limit":1,"sortDirection":"desc","itemsView":"summary"});

		params
	}

	pub(super) fn resume_params(thread: &str) -> Value {
		serde_json::json!({"threadId":thread,"excludeTurns":true})
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

async fn sample_needs_resume(client: &AppServerClient, thread: &str) -> Result<bool, ClientError> {
	let metadata = client.thread_read(serde_json::json!({"threadId":thread})).await?;
	if metadata["thread"]["id"] != thread {
		return Err(ClientError::InvalidFrame);
	}
	if metadata["thread"]["status"]["type"] != "notLoaded" {
		return Ok(false);
	}
	// Resuming an active native goal can start work. Suggestions may load only
	// dormant conversations and must never continue a goal on the user's behalf.
	if client
		.thread_goal(thread)
		.await?
		.is_some_and(|goal| goal.status == NativeThreadGoalStatus::Active)
	{
		return Err(ClientError::InvalidFrame);
	}
	Ok(true)
}

#[cfg(test)]
#[path = "../agent_sample_loading_tests.rs"]
mod sample_tests;
