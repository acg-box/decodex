//! Native App inventory and configuration edits bound to an owned task source.
use crate::{
	chief_config_settings as shared, chief_host::ChiefHostError, chief_usage_estimate::Source,
};
use decodex_codex::app_server_client::{AppToolExposureSettings, ClientError, HistoryGuard};
use decodex_database::{ChiefAppSettingsAttempt, SqliteStore};
use decodex_protocol::{
	ChiefAppExposureResult as State, ChiefToolExposureSurface, EntityId, WireText,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};

struct Inspection {
	state: State,
	settings: AppToolExposureSettings,
	guard: HistoryGuard,
	prior: Option<decodex_database::ChiefAppSettingsReceipt>,
	scope: String,
}
async fn inspect(store: &SqliteStore, source: &Source, connector: &str) -> Option<Inspection> {
	let key = &source.key;
	if !store
		.chief_thread_is_owned(
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
	let native = source.client.thread_read(json!({"threadId":key.thread})).await.ok()?;
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
		crate::native_config_warning::record_settings_error(
			store,
			source,
			"App tool visibility settings could not be read",
			error,
		)
		.await;
	}
	let settings = response.ok()?;
	let scope = shared::digest(settings.config_file());
	shared::reconcile(store, source, cwd, &scope).await;
	let prior = store.chief_app_settings_receipt(scope.clone()).await.ok()?;
	let last = store.chief_config_receipt(scope.clone()).await.ok()?;
	let work = store.get_chief_work_item(key.work.clone()).await.ok()?;
	if work.codex_thread_id.as_deref() != Some(&key.thread) || !guard.is_live() {
		return None;
	}
	let identity = json!([
		key.work,
		key.thread,
		key.generation.as_str(),
		key.account.as_str(),
		key.revision,
		key.history_revision,
		connector,
		settings.fingerprint(),
		prior.as_ref().map(|r| (r.id, &r.state)),
		last.as_ref().map(shared::project)
	]);
	let token: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	let can_update = work.status != decodex_database::ChiefWorkStatus::Resolved
		&& !last.as_ref().is_some_and(shared::pending);
	let state = State::Available {
		work_id: EntityId::new(key.work.clone()).ok()?,
		connector_id: WireText::new(connector).ok()?,
		review_token: WireText::new(token).ok()?,
		effective: settings.effective.clone(),
		preference: settings.preference.clone(),
		can_update,
		last_outcome: last.as_ref().map(|p| shared::project(p).outcome),
	};
	if serde_json::to_vec(&state).ok()?.len() > 32 * 1024 {
		return None;
	}
	Some(Inspection { state, settings, guard, prior, scope })
}

pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F, connector: &str) -> State
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return State::Unavailable;
	};
	let result = tokio::time::timeout(
		std::time::Duration::from_secs(30),
		inspect(store, &before, connector),
	)
	.await
	.ok()
	.flatten();
	if source().await.is_none_or(|after| after.key != before.key) {
		return State::Unavailable;
	}
	result.filter(|r| r.guard.is_live()).map_or(State::Unavailable, |r| r.state)
}

pub(crate) struct Change<'a> {
	pub connector: &'a str,
	pub review: &'a str,
	pub omit: Option<Vec<ChiefToolExposureSurface>>,
	pub attempt: &'a str,
}

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	change: Change<'_>,
) -> Result<(), ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use ChiefHostError::{Rejected, Unknown};
	let before = source().await.ok_or(Rejected("App settings source is unavailable."))?;
	let inspected = tokio::time::timeout(
		std::time::Duration::from_secs(30),
		inspect(store, &before, change.connector),
	)
	.await
	.ok()
	.flatten()
	.ok_or(Rejected("Refresh the App inventory and settings."))?;
	let State::Available { review_token, can_update: true, .. } = &inspected.state else {
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
	let attempt = ChiefAppSettingsAttempt {
		owner: shared::owner(&before),
		request_event_id: None,
		scope: inspected.scope.clone(),
		connector: change.connector.into(),
		link: String::new(),
		field: "omit_tools_from".into(),
		value: preference.as_ref().map(|v| json!(v)),
		previous_value: inspected.settings.preference.as_ref().map(|v| json!(v)),
		config_version: inspected.settings.config_version().into(),
		review_token: change.review.into(),
		attempt_id: change.attempt.into(),
		previous_id: inspected.prior.as_ref().map(|r| r.id),
	};
	let reservation = store
		.reserve_chief_app_settings_attempt(attempt.clone())
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
					crate::native_config_warning::record_settings_error(
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
		.finish_chief_app_settings_attempt(
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
