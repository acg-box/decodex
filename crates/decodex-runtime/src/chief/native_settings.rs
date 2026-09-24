//! Restore native task configuration without applying creation defaults.
use super::{ChiefCoordinator, ChiefError, Value, json};

impl ChiefCoordinator {
	pub(super) async fn hydrate_dispatch_thread(&mut self, thread: &str) -> Result<(), ChiefError> {
		if !self.loaded_threads.contains(thread) {
			let response = self
				.client
				.thread_resume(Self::resume_params(thread))
				.await
				.map_err(|error| super::resume_error(error, thread))?;
			if !Self::hydrated_thread_matches(&response, thread) {
				return Err(ChiefError::Invalid("resumed thread settings are invalid".into()));
			}
			let last_turn = response
				.pointer("/thread/turns")
				.and_then(Value::as_array)
				.and_then(|turns| turns.last())
				.and_then(|turn| turn["id"].as_str())
				.map(str::to_owned);
			self.store.validate_chief_usage_resume(thread.to_owned(), last_turn).await?;
			self.expect_usage_replay(thread, &response);
			self.loaded_threads.insert(thread.to_owned());
			self.persist_permission_observation(thread).await?;
		}
		Ok(())
	}

	pub(super) async fn observe_settings_notification(
		&self,
		method: &str,
		params: &Value,
	) -> Result<bool, ChiefError> {
		if method != "thread/settings/updated" {
			return Ok(false);
		}
		if let Some(thread) = params["threadId"].as_str() {
			self.cancel_changed_capacity_selection(thread, &params["threadSettings"]).await?;
			self.persist_permission_observation(thread).await?;
		}
		Ok(true)
	}

	pub(super) async fn persist_permission_observation(
		&self,
		thread: &str,
	) -> Result<(), ChiefError> {
		crate::chief_permissions::persist_current(
			&self.store,
			&self.client,
			thread,
			self.native_generation.as_ref().map(|g| g.as_str().into()),
		)
		.await?;
		Ok(())
	}

	pub(super) fn resume_params(thread: &str) -> Value {
		json!({"threadId":thread,"excludeTurns":true,"experimentalRawEvents":true})
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
