//! Source-bound model selection; the native runtime owns capability filtering.
#[path = "chief_model_recovery.rs"] mod recovery;
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::{
	ClientError, HistoryGuard, NativeTaskModelSettings, ThreadModelSelection,
};
use decodex_database::{ChiefModelAttempt, SqliteStore};
use decodex_protocol::{
	ChiefCapabilitiesResult, ChiefModelOutcome as Outcome, ChiefModelResponse as Response,
	ChiefModelSelectionReceipt as Receipt, ChiefModelSelectionState as State, ConversationModel,
	ConversationReasoningEffort, EntityId, WireText,
};
pub(crate) use recovery::recover_ordinary_model;
use serde_json::json;
use sha2::{Digest as _, Sha256};

struct Inspection {
	state: State,
	settings_event: i64,
	guard: Option<HistoryGuard>,
}
fn outcome(value: &str) -> Option<Outcome> {
	Some(match value {
		"reserved" => Outcome::Reserved,
		"queued" => Outcome::Queued,
		"unknown" => Outcome::Unknown,
		"rejected" => Outcome::Rejected,
		"target_observed" => Outcome::TargetObserved,
		"superseded" => Outcome::Superseded,
		_ => return None,
	})
}
fn pending(
	model: &str,
	effort: Option<&str>,
	state: Outcome,
	last_receipt: Option<Receipt>,
) -> Option<Inspection> {
	Some(Inspection {
		state: State::Pending {
			model: ConversationModel::new(model).ok()?,
			effort: effort.map(ConversationReasoningEffort::new).transpose().ok()?,
			state,
			last_receipt,
		},
		settings_event: 0,
		guard: None,
	})
}
fn historical_receipt(history: &decodex_database::ChiefModelHistory) -> Option<Receipt> {
	Some(Receipt {
		model: ConversationModel::new(history.model.clone()).ok()?,
		effort: history.effort.as_deref().map(ConversationReasoningEffort::new).transpose().ok()?,
		manual: history.manual,
		response: match history.response.as_str() {
			"reserved" => Response::Reserved,
			"queued" => Response::Queued,
			"rejected" => Response::Rejected,
			"unknown" => Response::Unknown,
			_ => return None,
		},
		target_observed: history.target_observed,
		reconciled: history.reconciled,
	})
}
async fn selection_editable(
	store: &SqliteStore,
	source: &Source,
	work: &decodex_database::ChiefWorkItem,
) -> Option<bool> {
	let k = &source.key;
	let permission_pending = store
		.chief_permission_receipt(k.work.clone(), k.thread.clone())
		.await
		.ok()?
		.is_some_and(|r| matches!(r.state.as_str(), "reserved" | "queued" | "unknown"));
	Some(
		((work.dispatch_state == decodex_database::ChiefDispatchState::Idle
			&& work.active_turn_id.is_none())
			|| (work.dispatch_state == decodex_database::ChiefDispatchState::Running
				&& work.active_turn_id.is_some()))
			&& work.status != decodex_database::ChiefWorkStatus::Resolved
			&& !permission_pending
			&& !store
				.chief_plugin_receipt(k.work.clone(), k.thread.clone())
				.await
				.ok()?
				.is_some_and(|r| matches!(r.state.as_str(), "reserved" | "queued" | "unknown")),
	)
}
async fn inspect(store: &SqliteStore, source: &Source) -> Option<Inspection> {
	let k = &source.key;
	if !store
		.chief_thread_is_owned(k.work.clone(), k.thread.clone(), Some(k.generation.as_str().into()))
		.await
		.ok()?
	{
		return None;
	}
	let work = store.get_chief_work_item(k.work.clone()).await.ok()?;
	if work.codex_thread_id.as_deref() != Some(&k.thread) {
		return None;
	}
	persist_current(store, &source.client, &k.thread, Some(k.generation.as_str().into()))
		.await
		.ok()?;
	let history = store
		.chief_model_history(k.work.clone(), k.thread.clone(), k.generation.as_str().into())
		.await
		.ok()?;
	let last_receipt = match &history {
		Some(history) => Some(historical_receipt(history)?),
		None => None,
	};
	if let Some(legacy) = store
		.pending_chief_legacy_model_change(
			k.work.clone(),
			k.thread.clone(),
			k.generation.as_str().into(),
		)
		.await
		.ok()?
	{
		return pending(&legacy.model, Some(&legacy.effort), outcome(&legacy.state)?, last_receipt);
	}
	let prior = store.chief_model_receipt(k.work.clone(), k.thread.clone()).await.ok()?;
	let last_outcome = match &prior {
		Some(receipt) => Some(outcome(&receipt.state)?),
		None => None,
	};
	if let Some(prior) = &prior
		&& matches!(last_outcome, Some(Outcome::Reserved | Outcome::Queued | Outcome::Unknown))
	{
		return pending(
			&prior.attempt.model,
			prior.attempt.effort.as_deref(),
			last_outcome?,
			last_receipt,
		);
	}
	let (native, guard) = source.client.configured_task_models(&k.thread)?;
	let saved = store
		.chief_task_models(k.work.clone(), k.thread.clone(), Some(k.generation.as_str().into()))
		.await
		.ok()??;
	let facts: NativeTaskModelSettings =
		serde_json::from_str(saved.settings_json.as_ref()?).ok()?;
	if facts != native || !guard.is_live() {
		return None;
	}
	let ChiefCapabilitiesResult::Available { mut models, .. } =
		crate::chief_capabilities::read(&source.client).await
	else {
		return None;
	};
	models.retain(|model| model.model.as_str() != "gpt-reserve");
	if !guard.is_live() {
		return None;
	}
	let can_update = selection_editable(store, source, &work).await?;

	let identity = json!([
		k.work,
		k.thread,
		k.generation.as_str(),
		k.account.as_str(),
		k.revision,
		k.history_revision,
		saved.id,
		native,
		models,
		prior.as_ref().map(|r| r.id),
		last_outcome,
		history.as_ref().map(|receipt| receipt.id),
		last_receipt,
		can_update
	]);
	let token: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	Some(Inspection {
		settings_event: saved.id,
		guard: Some(guard),
		state: State::Available {
			work_id: EntityId::new(k.work.clone()).ok()?,
			thread_id: EntityId::new(k.thread.clone()).ok()?,
			review_token: WireText::new(token).ok()?,
			model: ConversationModel::new(native.model).ok()?,
			model_provider: WireText::new(native.model_provider).ok()?,
			effort: native
				.effort
				.as_deref()
				.map(ConversationReasoningEffort::new)
				.transpose()
				.ok()?,
			models,
			can_update,
			last_outcome,
			last_receipt,
		},
	})
}
pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F) -> State
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return State::Unavailable;
	};
	let result = tokio::time::timeout(std::time::Duration::from_secs(30), inspect(store, &before))
		.await
		.ok()
		.flatten();
	if source().await.is_none_or(|after| after.key != before.key) {
		return State::Unavailable;
	}
	result
		.filter(|r| r.guard.as_ref().is_none_or(HistoryGuard::is_live))
		.map_or(State::Unavailable, |r| r.state)
}

pub(crate) struct Change<'a> {
	pub thread: &'a str,
	pub review: &'a str,
	pub model: &'a str,
	pub effort: Option<&'a str>,
	pub attempt_id: &'a str,
}
pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	change: Change<'_>,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::chief_host::ChiefHostError::{Rejected, Unknown};
	let before = source().await.ok_or(Rejected("The task source is unavailable."))?;
	if before.key.thread != change.thread {
		return Err(Rejected("The task thread changed. Refresh model settings."));
	}
	let inspected =
		tokio::time::timeout(std::time::Duration::from_secs(30), inspect(store, &before))
			.await
			.ok()
			.flatten()
			.ok_or(Rejected("Current model selection is unavailable."))?;
	let State::Available { review_token, model_provider, effort, models, can_update, .. } =
		&inspected.state
	else {
		return Err(Rejected("A model selection remains unconfirmed."));
	};
	let advertised = models.iter().any(|model| {
		model.model.as_str() == change.model
			&& change.effort.is_none_or(|effort| model.efforts.iter().any(|e| e.as_str() == effort))
	});
	if review_token.as_str() != change.review || !can_update || !advertised {
		return Err(Rejected("The reviewed model selection changed. Refresh the task."));
	}
	let selected_effort = change.effort.map(str::to_owned);
	let expected_effort =
		selected_effort.clone().or_else(|| effort.as_ref().map(|e| e.as_str().to_owned()));
	let selection = ThreadModelSelection::new(change.thread, change.model, selected_effort)
		.map_err(|_| Rejected("Invalid model selection."))?;
	let guard = inspected.guard.ok_or(Rejected("Current model evidence is unavailable."))?;
	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The task source changed before selection."));
	}
	let attempt = ChiefModelAttempt {
		work: before.key.work.clone(),
		thread: change.thread.into(),
		generation: Some(before.key.generation.as_str().into()),
		settings_event: inspected.settings_event,
		model: change.model.into(),
		model_provider: model_provider.as_str().into(),
		effort: expected_effort,
		review_token: change.review.into(),
		attempt_id: change.attempt_id.into(),
		manual_source: Some(decodex_database::ChiefManualModelSource {
			account: before.key.account.as_str().into(),
			account_revision: before.key.revision,
		}),
		recovery: None,
	};
	let reservation = store
		.reserve_chief_model_selection(attempt.clone())
		.await
		.map_err(|_| Unknown("The selection reservation is unconfirmed. Refresh saved state."))?
		.ok_or(Rejected("This review was used or the task is no longer editable."))?;
	let response = if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key)
	{
		Err(ClientError::StaleHistory)
	} else {
		before.client.queue_thread_model_selection(&selection, guard).await
	};
	let state = match response {
		Ok(_) => "queued",
		Err(
			ClientError::StaleHistory
			| ClientError::RequestTooLarge
			| ClientError::RequestQueueFull,
		) => "rejected",
		Err(ClientError::Remote(ref e)) if matches!(e.code, -32602..=-32600) => "rejected",
		_ => "unknown",
	};
	if !store
		.finish_chief_model_selection(reservation, attempt, state.into())
		.await
		.unwrap_or(false)
	{
		return Err(Unknown("The selection result could not be saved. It will not be retried."));
	}
	match state {
		"queued" => Ok(()),
		"rejected" => Err(Rejected("Native policy or changed source rejected the selection.")),
		_ => Err(Unknown("Model selection is unconfirmed. It will not be retried automatically.")),
	}
}

/// Save only transport-current facts. Missing facts invalidate the saved observation without
/// settling a selection. Receipt settlement records an observation, not request causation.
pub(crate) async fn persist_current(
	store: &SqliteStore,
	client: &decodex_codex::app_server_client::AppServerClient,
	thread: &str,
	generation: Option<String>,
) -> Result<(), decodex_database::StoreError> {
	let observed = client.configured_task_models(thread);
	let current = observed.as_ref().is_some_and(|(_, guard)| guard.is_live());
	let settings_revision = observed.as_ref().and_then(|(_, guard)| guard.settings_revision());
	let settings = observed.map(|(facts, _)| facts);
	let encoded = settings.as_ref().map(|facts| serde_json::to_string(facts).expect("model facts"));
	let identity =
		json!([generation, thread, client.history_revision(), settings_revision, settings]);
	let digest: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	if current {
		store
			.record_chief_task_models_publication(thread.into(), generation, encoded, digest)
			.await?;
	} else {
		store.record_chief_task_models(thread.into(), generation, None, digest).await?;
	}
	Ok(())
}
