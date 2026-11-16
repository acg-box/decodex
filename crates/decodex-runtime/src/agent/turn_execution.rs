//! Retain the requested selection, rather than substituting startup defaults on retry.
use crate::agent::{AgentCoordinator, AgentError, ClientError, HistoryGuard, Value};
use decodex_database::{AgentTurnExecution, AgentWorkItem};

impl AgentCoordinator {
	pub(super) async fn select_turn_execution(
		&self,
		params: &mut Value,
		guard: HistoryGuard,
	) -> Result<Option<AgentTurnExecution>, AgentError> {
		let mut inherited_tier = None;
		if params.get("model").is_none()
			|| params.get("effort").is_none()
			|| params.get("serviceTierForTurn").is_none()
		{
			let thread = params["threadId"]
				.as_str()
				.ok_or_else(|| AgentError::Invalid("missing thread".into()))?;
			let Some(settings) = self.client.thread_model_settings(thread, guard.clone()).await?
			else {
				return Ok(None);
			};
			inherited_tier = settings.service_tier.clone();

			// Bind inherited fields from the native task, never startup defaults.
			// Preserve every deliberate field in a partial user selection.
			if params.get("model").is_none() {
				let Some(model) = settings.model else { return Ok(None) };

				params["model"] = serde_json::json!(model);
			}
			if params.get("effort").is_none()
				&& let Some(effort) = settings.reasoning_effort
			{
				params["effort"] = serde_json::json!(effort);
			}
		}
		if let Some(tier) = params["serviceTierForTurn"]
			.as_str()
			.or_else(|| params["serviceTier"].as_str())
			.or(inherited_tier.as_deref())
			&& !matches!(tier, "default" | "flex")
			&& let Some(thread) = params["threadId"].as_str()
			&& let Some((catalog, _)) =
				crate::agent_capabilities::read_available_catalog(&self.client, Some(thread)).await
			&& let Some(model) = catalog
				.models
				.iter()
				.find(|model| Some(model.model.as_str()) == params["model"].as_str())
			&& let Ok(requested) = decodex_protocol::ServiceTier::new(tier)
		{
			let effective = model.effective_service_tier(&requested);
			if effective != requested {
				// Replace the removed speed for this thread and its next turn together.
				params["serviceTier"] = serde_json::json!(effective.thread_value());
			}
			params["serviceTierForTurn"] = serde_json::json!(effective);
		}
		if !guard.is_live() {
			return Err(ClientError::StaleHistory.into());
		}

		Ok(Some(AgentTurnExecution {
			model: params["model"]
				.as_str()
				.ok_or_else(|| AgentError::Invalid("invalid selected model".into()))?
				.into(),
			effort: params["effort"].as_str().map(str::to_owned),
		}))
	}

	pub(super) async fn validate_capacity_execution(
		&self,
		item: &AgentWorkItem,
		event: i64,
		execution: Option<&AgentTurnExecution>,
	) -> Result<(), AgentError> {
		let pending = self.store.pending_agent_capacity_retry(item.id.clone()).await?;
		let pending =
			pending.filter(|p| p.event_id == event).ok_or(AgentError::CapacityRetrySuperseded)?;
		let thread = item.codex_thread_id.clone().ok_or(AgentError::CapacityRetrySuperseded)?;
		let expected = self
			.store
			.agent_turn_execution(item.id.clone(), thread, pending.failed_turn_id)
			.await?;

		if expected.is_none() || execution != expected.as_ref() {
			self.store.cancel_agent_capacity_retry(item.id.clone(), event).await?;

			return Err(AgentError::CapacityRetrySuperseded);
		}

		Ok(())
	}

	pub(super) async fn cancel_changed_capacity_selection(
		&self,
		thread: &str,
		settings: &Value,
	) -> Result<(), AgentError> {
		let work = self
			.store
			.list_agent_work_items()
			.await?
			.into_iter()
			.find(|w| w.codex_thread_id.as_deref() == Some(thread));
		let Some(work) = work else { return Ok(()) };

		if !self
			.store
			.agent_thread_is_owned(
				work.id.clone(),
				thread.into(),
				self.native_generation.as_ref().map(|g| g.as_str().to_owned()),
			)
			.await?
		{
			return Ok(());
		}

		let Some(retry) = self.store.pending_agent_capacity_retry(work.id.clone()).await? else {
			return Ok(());
		};
		let saved = self
			.store
			.agent_turn_execution(work.id.clone(), thread.into(), retry.failed_turn_id)
			.await?;
		let matches = saved.is_some_and(|s| {
			settings["model"].as_str() == Some(s.model.as_str())
				&& settings.get("effort").is_some()
				&& settings["effort"].as_str() == s.effort.as_deref()
		});

		if !matches {
			self.store.cancel_agent_capacity_retry(work.id, retry.event_id).await?;
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use crate::agent::tests::fixture_with_history;

	#[tokio::test]
	async fn removed_speed_uses_the_native_default_and_supported_choices_stay_unchanged() {
		for (tiers, default, expected) in [
			(vec!["priority"], None, "default"),
			(vec!["priority"], Some("priority"), "priority"),
			(vec!["priority", "ultrafast"], Some("priority"), "ultrafast"),
		] {
			let (coordinator, _sent, _directory) =
				fixture_with_history(serde_json::json!({"_models":[{
					"model":"selected-model","displayName":"Selected",
					"supportedReasoningEfforts":[{"reasoningEffort":"high"}],"defaultReasoningEffort":"high",
					"serviceTiers":tiers.iter().map(|tier| serde_json::json!({"id":tier})).collect::<Vec<_>>(),
					"defaultServiceTier":default
				}]}))
				.await;
			let mut params = serde_json::json!({"threadId":"thread","model":"selected-model","effort":"high","serviceTier":"ultrafast","serviceTierForTurn":"ultrafast"});
			let guard = coordinator.client.history_guard(0).unwrap();
			coordinator.select_turn_execution(&mut params, guard).await.unwrap().unwrap();
			assert_eq!(params["model"], "selected-model");
			assert_eq!(params["effort"], "high");
			assert_eq!(params["serviceTierForTurn"], expected);
			assert_eq!(
				params["serviceTier"],
				if expected == "default" {
					serde_json::Value::Null
				} else {
					serde_json::json!(expected)
				}
			);
		}
	}
}
