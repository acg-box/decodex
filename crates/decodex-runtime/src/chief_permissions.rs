//! Review native profiles and reserve a source-bound selection before its only write.
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::{
	ClientError, HistoryGuard, NativeTaskPermissions, ThreadPermissionSelection,
};
use decodex_database::{ChiefPermissionAttempt, SqliteStore};
use decodex_protocol::{
	ChiefPermissionOutcome as Outcome, ChiefPermissionProfile, ChiefPermissionState as State,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};

struct Inspection {
	state: State,
	settings_event: i64,
	guard: Option<HistoryGuard>,
}
fn outcome(value: &str) -> Option<Outcome> {
	match value {
		"reserved" => Some(Outcome::Reserved),
		"queued" => Some(Outcome::Queued),
		"unknown" => Some(Outcome::Unknown),
		"rejected" => Some(Outcome::Rejected),
		"target_observed" => Some(Outcome::TargetObserved),
		"superseded" => Some(Outcome::Superseded),
		_ => None,
	}
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
	let prior = store.chief_permission_receipt(k.work.clone(), k.thread.clone()).await.ok()?;
	let last_outcome = match &prior {
		Some(receipt) => Some(outcome(&receipt.state)?),
		None => None,
	};
	if let Some(prior) = &prior
		&& matches!(last_outcome, Some(Outcome::Reserved | Outcome::Queued | Outcome::Unknown))
	{
		return Some(Inspection {
			state: State::Pending {
				profile_id: decodex_protocol::WireText::new(prior.attempt.profile.clone()).ok()?,
				state: last_outcome?,
			},
			settings_event: 0,
			guard: None,
		});
	}
	let (native, guard) = source.client.configured_task_permissions(&k.thread)?;
	let saved = store
		.chief_task_permissions(
			k.work.clone(),
			k.thread.clone(),
			Some(k.generation.as_str().into()),
		)
		.await
		.ok()??;
	let facts: NativeTaskPermissions = serde_json::from_str(saved.settings_json.as_ref()?).ok()?;
	if native != facts || !guard.is_live() {
		return None;
	}
	let profiles = match source.client.permission_profiles(&native.cwd).await {
		Ok(profiles) => profiles,
		Err(ClientError::Remote(error)) if error.code == -32601 =>
			return Some(Inspection { state: State::Unsupported, settings_event: 0, guard: None }),
		_ => return None,
	};
	if !guard.is_live() {
		return None;
	}
	let idle = work.dispatch_state == decodex_database::ChiefDispatchState::Idle
		&& work.active_turn_id.is_none();
	let running = work.dispatch_state == decodex_database::ChiefDispatchState::Running
		&& work.active_turn_id.is_some();
	let can_update =
		(idle || running) && work.status != decodex_database::ChiefWorkStatus::Resolved;
	let profiles: Vec<ChiefPermissionProfile> = profiles
		.into_iter()
		.map(|p| {
			Some(ChiefPermissionProfile {
				can_select: p.allowed && can_update,
				id: decodex_protocol::WireText::new(p.id).ok()?,
				allowed: p.allowed,
				description: p.description.map(decodex_protocol::WireText::new).transpose().ok()?,
			})
		})
		.collect::<Option<_>>()?;

	let identity = json!([
		k.work,
		k.thread,
		k.generation.as_str(),
		k.account.as_str(),
		k.revision,
		k.history_revision,
		saved.id,
		native,
		profiles,
		prior.as_ref().map(|p| p.id),
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
			work_id: decodex_protocol::EntityId::new(k.work.clone()).ok()?,
			thread_id: decodex_protocol::EntityId::new(k.thread.clone()).ok()?,
			review_token: decodex_protocol::WireText::new(token).ok()?,
			cwd: decodex_protocol::WireText::new(native.cwd).ok()?,
			profile_id: native.profile_id.map(decodex_protocol::WireText::new).transpose().ok()?,
			approvals_reviewer: decodex_protocol::WireText::new(native.approvals_reviewer).ok()?,
			profiles,
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
	let result = inspect(store, &before).await;
	if source().await.is_none_or(|after| after.key != before.key) {
		return State::Unavailable;
	}
	result
		.filter(|r| r.guard.as_ref().is_none_or(HistoryGuard::is_live))
		.map_or(State::Unavailable, |r| r.state)
}

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	thread: &str,
	review: &str,
	profile: &str,
	attempt_id: &str,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::chief_host::ChiefHostError::{Rejected, Unknown};
	let before = source().await.ok_or(Rejected("The task source is unavailable."))?;
	if before.key.thread != thread {
		return Err(Rejected("The task thread changed. Refresh its permissions."));
	}
	let inspected = inspect(store, &before)
		.await
		.ok_or(Rejected("Current native permissions are unavailable. Refresh the task."))?;
	let State::Available { review_token, profiles, can_update, profile_id, .. } = &inspected.state
	else {
		return Err(Rejected("A permission selection is unavailable or remains unconfirmed."));
	};
	if review_token.as_str() != review
		|| !can_update
		|| profile_id.as_ref().is_some_and(|p| p.as_str() == profile)
		|| !profiles.iter().any(|p| p.id.as_str() == profile && p.allowed && p.can_select)
	{
		return Err(Rejected(
			"The reviewed permissions or profile eligibility changed. Refresh the task.",
		));
	}
	let guard = inspected.guard.ok_or(Rejected("Current permission evidence is unavailable."))?;
	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The task source changed before selection."));
	}
	let selection = ThreadPermissionSelection::new(thread, profile)
		.map_err(|_| Rejected("Invalid permission selection."))?;
	let attempt = ChiefPermissionAttempt {
		work: before.key.work.clone(),
		thread: thread.into(),
		generation: Some(before.key.generation.as_str().into()),
		settings_event: inspected.settings_event,
		profile: profile.into(),
		review_token: review.into(),
		attempt_id: attempt_id.into(),
	};
	let reservation = store
		.reserve_chief_permission_selection(attempt.clone())
		.await
		.map_err(|_| Unknown("The selection could not be reserved. Refresh its saved state."))?
		.ok_or(Rejected("This review was already used or the task is no longer editable."))?;
	let response = if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key)
	{
		Err(ClientError::StaleHistory)
	} else {
		before.client.queue_thread_permission_selection(&selection, guard).await
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
		.finish_chief_permission_selection(reservation, attempt, state.into())
		.await
		.unwrap_or(false)
	{
		return Err(Unknown("The selection result could not be saved. It will not be retried."));
	}
	match state {
		"queued" => Ok(()),
		"rejected" => Err(Rejected("Native policy or a source change rejected the selection.")),
		_ => Err(Unknown(
			"Permission selection is unconfirmed. It will not be retried automatically.",
		)),
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
	let observed = client.configured_task_permissions(thread);
	let current = observed.as_ref().is_some_and(|(_, guard)| guard.is_live());
	let settings = observed.map(|(facts, _)| facts);
	let encoded =
		settings.as_ref().map(|facts| serde_json::to_string(facts).expect("permission facts"));
	let identity = json!([generation, thread, client.history_revision(), settings]);
	let digest: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	if current {
		store
			.record_chief_task_permissions_publication(thread.into(), generation, encoded, digest)
			.await?;
	} else {
		store.record_chief_task_permissions(thread.into(), generation, None, digest).await?;
	}
	Ok(())
}
