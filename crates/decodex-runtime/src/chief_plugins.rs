//! Source-bound plugin selection; the native runtime owns capability filtering.
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::{
	ClientError, HistoryGuard, NativeTaskPlugins, ThreadPluginSelection,
};
use decodex_database::{ChiefPluginAttempt, SqliteStore};
use decodex_protocol::{
	ChiefPluginInventory, ChiefPluginOutcome as Outcome, ChiefPluginSelectionState as State,
	EntityId, WireText,
};
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
	let prior = store.chief_plugin_receipt(k.work.clone(), k.thread.clone()).await.ok()?;
	let last_outcome = match &prior {
		Some(receipt) => Some(outcome(&receipt.state)?),
		None => None,
	};
	if let Some(prior) = &prior
		&& matches!(last_outcome, Some(Outcome::Reserved | Outcome::Queued | Outcome::Unknown))
	{
		return Some(Inspection {
			state: State::Pending {
				disabled_plugin_ids: prior
					.attempt
					.disabled_plugin_ids
					.iter()
					.cloned()
					.map(WireText::new)
					.collect::<Result<_, _>>()
					.ok()?,
				state: last_outcome?,
			},
			settings_event: 0,
			guard: None,
		});
	}
	let (native, guard) = source.client.configured_task_plugins(&k.thread)?;
	let saved = store
		.chief_task_plugins(k.work.clone(), k.thread.clone(), Some(k.generation.as_str().into()))
		.await
		.ok()??;
	let facts: NativeTaskPlugins = serde_json::from_str(saved.settings_json.as_ref()?).ok()?;
	if facts != native || !guard.is_live() {
		return None;
	}
	let catalog = read_catalog(&source.client, &k.thread).await?;
	if !guard.is_live() {
		return None;
	}
	let other_pending = store
		.chief_permission_receipt(k.work.clone(), k.thread.clone())
		.await
		.ok()?
		.is_some_and(|r| matches!(r.state.as_str(), "reserved" | "queued" | "unknown"));
	let can_update = ((work.dispatch_state == decodex_database::ChiefDispatchState::Idle
		&& work.active_turn_id.is_none())
		|| (work.dispatch_state == decodex_database::ChiefDispatchState::Running
			&& work.active_turn_id.is_some()))
		&& work.status != decodex_database::ChiefWorkStatus::Resolved
		&& !other_pending
		&& !store
			.chief_model_receipt(k.work.clone(), k.thread.clone())
			.await
			.ok()?
			.is_some_and(|r| matches!(r.state.as_str(), "reserved" | "queued" | "unknown"));

	let identity = json!([
		k.work,
		k.thread,
		k.generation.as_str(),
		k.account.as_str(),
		k.revision,
		k.history_revision,
		saved.id,
		native,
		catalog,
		prior.as_ref().map(|r| r.id),
		last_outcome,
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
			disabled_plugin_ids: native
				.disabled_plugin_ids
				.into_iter()
				.map(WireText::new)
				.collect::<Result<_, _>>()
				.ok()?,
			catalog,
			can_update,
			last_outcome,
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
	pub plugin: &'a str,
	pub enabled: bool,
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
		return Err(Rejected("The task thread changed. Refresh plugin settings."));
	}
	let inspected =
		tokio::time::timeout(std::time::Duration::from_secs(30), inspect(store, &before))
			.await
			.ok()
			.flatten()
			.ok_or(Rejected("Current plugin selection is unavailable."))?;
	let State::Available { review_token, disabled_plugin_ids, catalog, can_update, .. } =
		&inspected.state
	else {
		return Err(Rejected("A plugin selection remains unconfirmed."));
	};
	let excluded = disabled_plugin_ids.iter().any(|id| id.as_str() == change.plugin);
	let installed = matches!(catalog,ChiefPluginInventory::Available{plugins,..} if plugins.iter().any(|p|p.id==change.plugin && p.installed));
	if review_token.as_str() != change.review
		|| !can_update
		|| excluded != change.enabled
		|| (!excluded && !installed)
	{
		return Err(Rejected("The reviewed plugin selection changed. Refresh the task."));
	}
	let mut target: Vec<String> =
		disabled_plugin_ids.iter().map(|id| id.as_str().to_owned()).collect();
	if change.enabled {
		target.retain(|id| id != change.plugin);
	} else {
		target.push(change.plugin.into());
	}
	let selection = ThreadPluginSelection::new(change.thread, target.clone())
		.map_err(|_| Rejected("Invalid plugin selection."))?;
	let guard = inspected.guard.ok_or(Rejected("Current plugin evidence is unavailable."))?;
	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The task source changed before selection."));
	}
	let attempt = ChiefPluginAttempt {
		work: before.key.work.clone(),
		thread: change.thread.into(),
		generation: Some(before.key.generation.as_str().into()),
		settings_event: inspected.settings_event,
		disabled_plugin_ids: target,
		review_token: change.review.into(),
		attempt_id: change.attempt_id.into(),
	};
	let reservation = store
		.reserve_chief_plugin_selection(attempt.clone())
		.await
		.map_err(|_| Unknown("The selection reservation is unconfirmed. Refresh saved state."))?
		.ok_or(Rejected("This review was used or the task is no longer editable."))?;
	let response = if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key)
	{
		Err(ClientError::StaleHistory)
	} else {
		before.client.queue_thread_plugin_selection(&selection, guard).await
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
		.finish_chief_plugin_selection(reservation, attempt, state.into())
		.await
		.unwrap_or(false)
	{
		return Err(Unknown("The selection result could not be saved. It will not be retried."));
	}
	match state {
		"queued" => Ok(()),
		"rejected" => Err(Rejected("Native policy or changed source rejected the selection.")),
		_ => Err(Unknown("Plugin selection is unconfirmed. It will not be retried automatically.")),
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
	let observed = client.configured_task_plugins(thread);
	let current = observed.as_ref().is_some_and(|(_, guard)| guard.is_live());
	let settings_revision = observed.as_ref().and_then(|(_, guard)| guard.settings_revision());
	let settings = observed.map(|(facts, _)| facts);
	let encoded =
		settings.as_ref().map(|facts| serde_json::to_string(facts).expect("plugin facts"));
	let identity =
		json!([generation, thread, client.history_revision(), settings_revision, settings]);
	let digest: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	if current {
		store
			.record_chief_task_plugins_publication(thread.into(), generation, encoded, digest)
			.await?;
	} else {
		store.record_chief_task_plugins(thread.into(), generation, None, digest).await?;
	}
	Ok(())
}

async fn read_catalog(
	client: &decodex_codex::app_server_client::AppServerClient,
	thread: &str,
) -> Option<ChiefPluginInventory> {
	let read = client.thread_read(json!({"threadId":thread,"includeTurns":false})).await.ok()?;
	if read["thread"]["id"].as_str() != Some(thread) {
		return None;
	}
	let cwd = read["thread"]["cwd"].as_str()?;
	if !std::path::Path::new(cwd).is_absolute() {
		return None;
	}
	let mut catalog = crate::chief_integrations::project_plugins(
		client.installed_plugins_for_directory(cwd).await,
	);
	if serde_json::to_vec(&catalog).ok()?.len() > 64 * 1024 {
		catalog = ChiefPluginInventory::CapacityExceeded;
	}
	Some(catalog)
}
