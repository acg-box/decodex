//! Native App inventory and configuration edits bound to an owned task source.
use std::{future::Future, time::Duration};

use AgentHostError::{Rejected, Unknown};
use sha2::{Digest as _, Sha256};
use tokio::time;

use crate::{
	agent_config_settings, agent_host::AgentHostError, agent_usage_estimate::Source,
	native_config_warning,
};
use decodex_codex::app_server_client::{AppToolExposureSettings, ClientError, HistoryGuard};
use decodex_database::{
	AgentAppSettingsAttempt, AgentAppSettingsReceipt, AgentWorkStatus, SqliteStore,
};
use decodex_protocol::{AgentToolExposureSurface, EntityId, WireText};

pub(crate) struct Change<'a> {
	pub connector: &'a str,
	pub review: &'a str,
	pub omit: Option<Vec<AgentToolExposureSurface>>,
	pub attempt: &'a str,
}

struct Inspection {
	state: decodex_protocol::AgentAppExposureResult,
	settings: AppToolExposureSettings,
	guard: HistoryGuard,
	prior: Option<AgentAppSettingsReceipt>,
	scope: String,
}

pub(crate) async fn read<F, Fut>(
	store: &SqliteStore,
	source: F,
	connector: &str,
) -> decodex_protocol::AgentAppExposureResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return decodex_protocol::AgentAppExposureResult::Unavailable;
	};
	let result = time::timeout(Duration::from_secs(30), inspect(store, &before, connector))
		.await
		.ok()
		.flatten();

	if source().await.is_none_or(|after| after.key != before.key) {
		return decodex_protocol::AgentAppExposureResult::Unavailable;
	}

	result
		.filter(|r| r.guard.is_live())
		.map_or(decodex_protocol::AgentAppExposureResult::Unavailable, |r| r.state)
}

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	change: Change<'_>,
) -> Result<(), AgentHostError>
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let before = source().await.ok_or(Rejected("App settings source is unavailable."))?;
	let inspected =
		time::timeout(Duration::from_secs(30), inspect(store, &before, change.connector))
			.await
			.ok()
			.flatten()
			.ok_or(Rejected("Refresh the App inventory and settings."))?;
	let decodex_protocol::AgentAppExposureResult::Available {
		review_token, can_update: true, ..
	} = &inspected.state
	else {
		return Err(Rejected("This settings review was already submitted or is not editable."));
	};

	if review_token.as_str() != change.review
		|| !inspected.guard.is_live()
		|| source().await.is_none_or(|after| after.key != before.key)
	{
		return Err(Rejected("App settings or their source changed. Refresh them."));
	}

	let preference =
		change.omit.map(|v| v.into_iter().map(|v| v.as_str().to_owned()).collect::<Vec<_>>());

	if preference == inspected.settings.preference {
		return Err(Rejected("This preference is already stored."));
	}

	let attempt = AgentAppSettingsAttempt {
		owner: agent_config_settings::owner(&before),
		request_event_id: None,
		scope: inspected.scope.clone(),
		connector: change.connector.into(),
		link: String::new(),
		field: "omit_tools_from".into(),
		value: preference.as_ref().map(|v| serde_json::json!(v)),
		previous_value: inspected.settings.preference.as_ref().map(|v| serde_json::json!(v)),
		config_version: inspected.settings.config_version().into(),
		review_token: change.review.into(),
		attempt_id: change.attempt.into(),
		previous_id: inspected.prior.as_ref().map(|r| r.id),
	};
	let reservation = store
		.reserve_agent_app_settings_attempt(attempt.clone())
		.await
		.map_err(|_| Unknown("The write reservation is unconfirmed. Refresh saved settings."))?
		.ok_or(Rejected("This review was already used or the task source changed."))?;
	let mut saved_version = None;
	let (state, result) = if !inspected.guard.is_live()
		|| source().await.is_none_or(|after| after.key != before.key)
	{
		("rejected", Err(Rejected("The task source changed before the write.")))
	} else {
		match before
			.client
			.write_app_tool_exposure(&inspected.settings, preference.clone(), inspected.guard)
			.await
		{
			Ok(saved)
				if saved.settings.preference == preference
					&& source().await.is_some_and(|after| after.key == before.key) =>
			{
				saved_version = Some(saved.settings.config_version().to_owned());

				(if saved.overridden { "overridden" } else { "saved" }, Ok(()))
			},
			Err(ClientError::StaleHistory) =>
				("rejected", Err(Rejected("The native source changed before dispatch."))),
			Err(error) => {
				if source().await.is_some_and(|after| after.key == before.key) {
					native_config_warning::record_settings_error(
						store,
						&before,
						"App tool visibility write or readback failed",
						&error,
					)
					.await;
				}

				(
					"unknown",
					Err(Unknown(
						"App setting write or readback is unconfirmed. Read task diagnostics and refresh; it will not be retried.",
					)),
				)
			},
			_ => (
				"unknown",
				Err(Unknown(
					"App setting write or readback is unconfirmed. Refresh; it will not be retried.",
				)),
			),
		}
	};

	if !store
		.finish_agent_app_settings_attempt(
			reservation,
			attempt.attempt_id,
			state.into(),
			saved_version,
		)
		.await
		.unwrap_or(false)
	{
		return Err(Unknown(
			"The write outcome could not be recorded. Refresh native settings; do not replay.",
		));
	}

	result
}

async fn inspect(store: &SqliteStore, source: &Source, connector: &str) -> Option<Inspection> {
	let key = &source.key;

	if !store
		.agent_thread_is_owned(
			key.work.clone(),
			key.thread.clone(),
			Some(key.generation.as_str().into()),
		)
		.await
		.ok()?
	{
		return None;
	}

	let guard = source.client.thread_settings_guard(&key.thread)?;
	let native =
		source.client.thread_read(serde_json::json!({"threadId":key.thread})).await.ok()?;

	if native["thread"]["id"] != key.thread {
		return None;
	}

	let apps = source.client.installed_apps_for_thread(&key.thread, false).await.ok()?;

	if apps.iter().filter(|app| app["id"].as_str() == Some(connector)).count() != 1 {
		return None;
	}

	let cwd = native["thread"]["cwd"].as_str()?;
	let response = source.client.app_tool_exposure(cwd, connector).await;

	if guard.is_live()
		&& let Err(error) = &response
	{
		native_config_warning::record_settings_error(
			store,
			source,
			"App tool visibility settings could not be read",
			error,
		)
		.await;
	}

	let settings = response.ok()?;
	let scope = agent_config_settings::digest(settings.config_file());

	agent_config_settings::reconcile(store, source, cwd, &scope).await;

	let prior = store.agent_app_settings_receipt(scope.clone()).await.ok()?;
	let last = store.agent_config_receipt(scope.clone()).await.ok()?;
	let legacy = if last.is_none() {
		store
			.legacy_agent_app_exposure_outcome(
				key.work.clone(),
				key.thread.clone(),
				connector.into(),
			)
			.await
			.ok()?
	} else {
		None
	};
	let work = store.get_agent_work_item(key.work.clone()).await.ok()?;

	if work.codex_thread_id.as_deref() != Some(&key.thread) || !guard.is_live() {
		return None;
	}

	let identity = serde_json::json!([
		key.work,
		key.thread,
		key.generation.as_str(),
		key.account.as_str(),
		key.revision,
		key.history_revision,
		connector,
		settings.fingerprint(),
		prior.as_ref().map(|r| (r.id, &r.state)),
		last.as_ref().map(crate::agent_config_settings::project),
		legacy
	]);
	let token: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	let can_update = work.status != AgentWorkStatus::Resolved
		&& !last.as_ref().is_some_and(crate::agent_config_settings::pending);
	let state = decodex_protocol::AgentAppExposureResult::Available {
		work_id: EntityId::new(key.work.clone()).ok()?,
		connector_id: WireText::new(connector).ok()?,
		review_token: WireText::new(token).ok()?,
		effective: settings.effective.clone(),
		preference: settings.preference.clone(),
		can_update,
		last_outcome: last
			.as_ref()
			.map(|p| agent_config_settings::project(p).outcome)
			.or_else(|| legacy.map(|(_, state)| state)),
	};

	if serde_json::to_vec(&state).ok()?.len() > 32 * 1_024 {
		return None;
	}

	Some(Inspection { state, settings, guard, prior, scope })
}
