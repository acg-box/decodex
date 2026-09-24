//! Retain the requested selection, rather than substituting startup defaults on retry.
use super::{ChiefCoordinator, ChiefError, Value, json};
use decodex_database::ChiefTurnExecution;

impl ChiefCoordinator {
	pub(super) async fn select_turn_execution(
		&self,
		params: &mut Value,
		guard: super::HistoryGuard,
	) -> Result<Option<ChiefTurnExecution>, ChiefError> {
		if params.get("model").is_none() || params.get("effort").is_none() {
			let thread = params["threadId"]
				.as_str()
				.ok_or_else(|| ChiefError::Invalid("missing thread".into()))?;
			let Some(settings) = self.client.thread_model_settings(thread, guard.clone()).await?
			else {
				return Ok(None);
			};
			// Bind inherited fields from the native task, never startup defaults.
			// Preserve every deliberate field in a partial user selection.
			if params.get("model").is_none() {
				let Some(model) = settings.model else { return Ok(None) };
				params["model"] = json!(model);
			}
			if params.get("effort").is_none()
				&& let Some(effort) = settings.reasoning_effort
			{
				params["effort"] = json!(effort);
			}
		}
		if !guard.is_live() {
			return Err(super::ClientError::StaleHistory.into());
		}
		Ok(Some(ChiefTurnExecution {
			model: params["model"]
				.as_str()
				.ok_or_else(|| ChiefError::Invalid("invalid selected model".into()))?
				.into(),
			effort: params["effort"].as_str().map(str::to_owned),
		}))
	}

	pub(super) async fn validate_capacity_execution(
		&self,
		item: &decodex_database::ChiefWorkItem,
		event: i64,
		execution: Option<&ChiefTurnExecution>,
	) -> Result<(), ChiefError> {
		let pending = self.store.pending_chief_capacity_retry(item.id.clone()).await?;
		let pending = pending.filter(|p| p.event_id == event).ok_or(ChiefError::Rejected(
			"The task model or effort changed; the saved capacity retry was cancelled.".into(),
		))?;
		let thread = item.codex_thread_id.clone().ok_or(ChiefError::Rejected(
			"The task model or effort changed; the saved capacity retry was cancelled.".into(),
		))?;
		let expected = self
			.store
			.chief_turn_execution(item.id.clone(), thread, pending.failed_turn_id)
			.await?;
		if expected.is_none() || execution != expected.as_ref() {
			self.store.cancel_chief_capacity_retry(item.id.clone(), event).await?;
			return Err(ChiefError::Rejected(
				"The task model or effort changed; the saved capacity retry was cancelled.".into(),
			));
		}
		Ok(())
	}

	pub(super) async fn cancel_changed_capacity_selection(
		&self,
		thread: &str,
		settings: &Value,
	) -> Result<(), ChiefError> {
		let work = self
			.store
			.list_chief_work_items()
			.await?
			.into_iter()
			.find(|w| w.codex_thread_id.as_deref() == Some(thread));
		let Some(work) = work else { return Ok(()) };
		if !self
			.store
			.chief_thread_is_owned(
				work.id.clone(),
				thread.into(),
				self.native_generation.as_ref().map(|g| g.as_str().to_owned()),
			)
			.await?
		{
			return Ok(());
		}
		let Some(retry) = self.store.pending_chief_capacity_retry(work.id.clone()).await? else {
			return Ok(());
		};
		let saved = self
			.store
			.chief_turn_execution(work.id.clone(), thread.into(), retry.failed_turn_id)
			.await?;
		let matches = saved.is_some_and(|s| {
			settings["model"].as_str() == Some(s.model.as_str())
				&& settings.get("effort").is_some()
				&& settings["effort"].as_str() == s.effort.as_deref()
		});
		if !matches {
			self.store.cancel_chief_capacity_retry(work.id, retry.event_id).await?;
		}
		Ok(())
	}
}
