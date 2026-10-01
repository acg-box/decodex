//! Review native profiles and reserve a source-bound selection before its only write.
use std::future::Future;

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{agent_host::AgentHostError, agent_usage_estimate::Source};
use decodex_codex::app_server_client::{
	AppServerClient, ClientError, HistoryGuard, NativeTaskPermissions, ThreadPermissionSelection,
};
use decodex_database::{
	AgentDispatchState, AgentPermissionAttempt, AgentWorkItem, AgentWorkStatus, SqliteStore,
	StoreError,
};
use decodex_protocol::{
	AgentPermissionOutcome, AgentPermissionProfile, AgentPermissionState, EntityId, WireText,
};

struct Inspection {
	state: AgentPermissionState,
	settings_event: i64,
	guard: Option<HistoryGuard>,
}

pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F) -> AgentPermissionState
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return AgentPermissionState::Unavailable;
	};
	let result = inspect(store, &before).await;

	if source().await.is_none_or(|after| after.key != before.key) {
		return AgentPermissionState::Unavailable;
	}

	result
		.filter(|r| r.guard.as_ref().is_none_or(HistoryGuard::is_live))
		.map_or(AgentPermissionState::Unavailable, |r| r.state)
}

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	thread: &str,
	review: &str,
	profile: &str,
	attempt_id: &str,
) -> Result<(), AgentHostError>
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let before =
		source().await.ok_or(AgentHostError::Rejected("The task source is unavailable."))?;

	if before.key.thread != thread {
		return Err(AgentHostError::Rejected("The task thread changed. Refresh its permissions."));
	}

	let inspected = inspect(store, &before).await.ok_or(AgentHostError::Rejected(
		"Current native permissions are unavailable. Refresh the task.",
	))?;
	let AgentPermissionState::Available { review_token, profiles, can_update, profile_id, .. } =
		&inspected.state
	else {
		return Err(AgentHostError::Rejected(
			"A permission selection is unavailable or remains unconfirmed.",
		));
	};

	if review_token.as_str() != review
		|| !can_update
		|| profile_id.as_ref().is_some_and(|p| p.as_str() == profile)
		|| !profiles.iter().any(|p| p.id.as_str() == profile && p.allowed && p.can_select)
	{
		return Err(AgentHostError::Rejected(
			"The reviewed permissions or profile eligibility changed. Refresh the task.",
		));
	}

	let guard = inspected
		.guard
		.ok_or(AgentHostError::Rejected("Current permission evidence is unavailable."))?;

	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return Err(AgentHostError::Rejected("The task source changed before selection."));
	}

	let selection = ThreadPermissionSelection::new(thread, profile)
		.map_err(|_| AgentHostError::Rejected("Invalid permission selection."))?;
	let attempt = AgentPermissionAttempt {
		work: before.key.work.clone(),
		thread: thread.into(),
		generation: Some(before.key.generation.as_str().into()),
		settings_event: inspected.settings_event,
		profile: profile.into(),
		review_token: review.into(),
		attempt_id: attempt_id.into(),
	};
	let reservation = store
		.reserve_agent_permission_selection(attempt.clone())
		.await
		.map_err(|_| {
			AgentHostError::Unknown("The selection could not be reserved. Refresh its saved state.")
		})?
		.ok_or(AgentHostError::Rejected(
			"This review was already used or the task is no longer editable.",
		))?;
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
		Err(ClientError::Remote(ref e)) if matches!(e.code, -32_602..=-32_600) => "rejected",
		_ => "unknown",
	};

	if !store
		.finish_agent_permission_selection(reservation, attempt, state.into())
		.await
		.unwrap_or(false)
	{
		return Err(AgentHostError::Unknown(
			"The selection result could not be saved. It will not be retried.",
		));
	}

	match state {
		"queued" => Ok(()),
		"rejected" => Err(AgentHostError::Rejected(
			"Native policy or a source change rejected the selection.",
		)),
		_ => Err(AgentHostError::Unknown(
			"Permission selection is unconfirmed. It will not be retried automatically.",
		)),
	}
}

/// Save only transport-current facts. Missing facts invalidate the saved observation without
/// settling a selection. Receipt settlement records an observation, not request causation.
pub(crate) async fn persist_current(
	store: &SqliteStore,
	client: &AppServerClient,
	thread: &str,
	generation: Option<String>,
) -> Result<(), StoreError> {
	let observed = client.configured_task_permissions(thread);
	let current = observed.as_ref().is_some_and(|(_, guard)| guard.is_live());
	let settings_revision = observed.as_ref().and_then(|(_, guard)| guard.settings_revision());
	let settings = observed.map(|(facts, _)| facts);
	let encoded =
		settings.as_ref().map(|facts| serde_json::to_string(facts).expect("permission facts"));
	let identity = serde_json::json!([
		generation,
		thread,
		client.history_revision(),
		settings_revision,
		settings
	]);
	let digest: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();

	if current {
		store
			.record_agent_task_permissions_publication(thread.into(), generation, encoded, digest)
			.await?;
	} else {
		store.record_agent_task_permissions(thread.into(), generation, None, digest).await?;
	}

	Ok(())
}

fn outcome(value: &str) -> Option<AgentPermissionOutcome> {
	match value {
		"reserved" => Some(AgentPermissionOutcome::Reserved),
		"queued" => Some(AgentPermissionOutcome::Queued),
		"unknown" => Some(AgentPermissionOutcome::Unknown),
		"rejected" => Some(AgentPermissionOutcome::Rejected),
		"target_observed" => Some(AgentPermissionOutcome::TargetObserved),
		"superseded" => Some(AgentPermissionOutcome::Superseded),
		_ => None,
	}
}

fn review_token(identity: &Value) -> String {
	Sha256::digest(identity.to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

async fn inspect(store: &SqliteStore, source: &Source) -> Option<Inspection> {
	let k = &source.key;

	if !store
		.agent_thread_is_owned(k.work.clone(), k.thread.clone(), Some(k.generation.as_str().into()))
		.await
		.ok()?
	{
		return None;
	}

	let work = store.get_agent_work_item(k.work.clone()).await.ok()?;

	if work.codex_thread_id.as_deref() != Some(&k.thread) {
		return None;
	}

	persist_current(store, &source.client, &k.thread, Some(k.generation.as_str().into()))
		.await
		.ok()?;

	let prior = store.agent_permission_receipt(k.work.clone(), k.thread.clone()).await.ok()?;
	let last_outcome = match &prior {
		Some(receipt) => Some(outcome(&receipt.state)?),
		None => None,
	};

	if let Some(prior) = &prior
		&& matches!(
			last_outcome,
			Some(
				AgentPermissionOutcome::Reserved
					| AgentPermissionOutcome::Queued
					| AgentPermissionOutcome::Unknown
			)
		) {
		return Some(Inspection {
			state: AgentPermissionState::Pending {
				profile_id: WireText::new(prior.attempt.profile.clone()).ok()?,
				state: last_outcome?,
			},
			settings_event: 0,
			guard: None,
		});
	}

	let (native, guard) = source.client.configured_task_permissions(&k.thread)?;
	let saved = store
		.agent_task_permissions(
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
		Err(ClientError::Remote(error)) if error.code == -32_601 =>
			return Some(Inspection {
				state: AgentPermissionState::Unsupported,
				settings_event: 0,
				guard: None,
			}),
		_ => return None,
	};

	if !guard.is_live() {
		return None;
	}

	let can_update = selection_editable(store, source, &work).await?;
	let profiles: Vec<AgentPermissionProfile> = profiles
		.into_iter()
		.map(|p| {
			Some(AgentPermissionProfile {
				can_select: p.allowed && can_update,
				id: WireText::new(p.id).ok()?,
				allowed: p.allowed,
				description: p.description.map(WireText::new).transpose().ok()?,
			})
		})
		.collect::<Option<_>>()?;
	let identity = serde_json::json!([
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
	let token = review_token(&identity);

	Some(Inspection {
		settings_event: saved.id,
		guard: Some(guard),
		state: AgentPermissionState::Available {
			work_id: EntityId::new(k.work.clone()).ok()?,
			thread_id: EntityId::new(k.thread.clone()).ok()?,
			review_token: WireText::new(token).ok()?,
			cwd: WireText::new(native.cwd).ok()?,
			profile_id: native.profile_id.map(WireText::new).transpose().ok()?,
			approvals_reviewer: WireText::new(native.approvals_reviewer).ok()?,
			profiles,
			can_update,
			last_outcome,
		},
	})
}

async fn selection_editable(
	store: &SqliteStore,
	source: &Source,
	work: &AgentWorkItem,
) -> Option<bool> {
	let k = &source.key;
	let idle = work.dispatch_state == AgentDispatchState::Idle && work.active_turn_id.is_none();
	let running =
		work.dispatch_state == AgentDispatchState::Running && work.active_turn_id.is_some();
	let other_pending = store
		.agent_plugin_receipt(k.work.clone(), k.thread.clone())
		.await
		.ok()?
		.is_some_and(|r| matches!(r.state.as_str(), "reserved" | "queued" | "unknown"));
	let can_update = (idle || running)
		&& !other_pending
		&& !store.has_pending_agent_model_change(k.work.clone()).await.ok()?
		&& work.status != AgentWorkStatus::Resolved;

	Some(can_update)
}
