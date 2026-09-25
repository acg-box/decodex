//! Native app settings with one-shot consent and independent durable write receipts.
use crate::{chief_config_settings as shared, chief_usage_estimate::Source};
use decodex_codex::app_server_client::{
	AppLinkSettingEdit, AppLinkSettings, ClientError, HistoryGuard, ServerRequestGuard,
};
use decodex_database::{ChiefAppSettingsAttempt, ChiefAppSettingsReceipt, SqliteStore};
use decodex_protocol::{ChiefAppSettingEdit, ChiefAppSettingsResult as State};
use serde_json::{Value, json};
enum SettingsGuard {
	Request(ServerRequestGuard),
	Saved(HistoryGuard),
}
impl SettingsGuard {
	fn is_live(&self) -> bool {
		match self {
			Self::Request(g) => g.is_live(),
			Self::Saved(g) => g.is_live(),
		}
	}
}
struct Review {
	state: State,
	native: AppLinkSettings,
	guard: Option<SettingsGuard>,
	prior: Option<ChiefAppSettingsReceipt>,
	scope: String,
}
async fn inspect<F, Fut>(store: &SqliteStore, source: &F, event_id: i64) -> Option<(Source, Review)>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let before = source().await?;
	let k = &before.key;
	let event = store.get_chief_inbox_event(event_id).await.ok()?;
	if event.work_item_id != k.work
		|| event.event_kind != "server_request_pending"
		|| !store
			.chief_thread_is_owned(
				k.work.clone(),
				k.thread.clone(),
				Some(k.generation.as_str().into()),
			)
			.await
			.ok()?
	{
		return None;
	}
	let payload: Value = serde_json::from_str(&event.payload).ok()?;
	let (connector, link) = account_identity(&payload)?;
	let params = &payload["params"];
	let thread = params["threadId"].as_str()?;
	if thread != k.thread {
		let owner = crate::chief::native_subagents::request_owner(store, &before.client, thread)
			.await
			.ok()?;
		if owner.id != k.work
			|| owner.codex_thread_id.as_deref() != Some(&k.thread)
			|| payload["ownerThreadId"].as_str() != Some(&k.thread)
		{
			return None;
		}
	}
	let id = serde_json::from_value(payload["id"].clone()).ok()?;
	let guard = before
		.client
		.server_request_guard(&id, "mcpServer/elicitation/request", params)
		.map(SettingsGuard::Request);
	let native_thread =
		before.client.thread_read(json!({"threadId":thread,"includeTurns":false})).await.ok()?;
	if native_thread["thread"]["id"] != thread {
		return None;
	}
	let cwd = native_thread["thread"]["cwd"].as_str()?;
	let response = before.client.app_link_settings(cwd, connector, link).await;
	if source().await.is_some_and(|after| after.key == before.key)
		&& let Err(error) = &response
	{
		crate::native_config_warning::record_settings_error(
			store,
			&before,
			"Connection settings could not be read",
			error,
		)
		.await;
	}
	let native = response.ok()?;
	let scope = shared::digest(native.config_file());
	if source().await.is_none_or(|after| after.key != before.key) {
		return None;
	}
	// Failure to read an uncertain target leaves its receipt pending and blocks another write.
	shared::reconcile(store, &before, cwd, &scope).await;
	let last = store.chief_config_receipt(scope.clone()).await.ok()?;
	let prior = store.chief_app_settings_receipt(scope.clone()).await.ok()?;
	let current = store.get_chief_inbox_event(event_id).await.ok()?;
	let can_update = current.disposition.is_none()
		&& guard.as_ref().is_some_and(|g| g.is_live())
		&& !last.as_ref().is_some_and(shared::pending);
	let token = shared::digest(
		&json!([
			k.work,
			k.thread,
			k.generation.as_str(),
			k.account.as_str(),
			k.revision,
			k.history_revision,
			event_id,
			payload,
			native.review_fingerprint(),
			last.as_ref().map(shared::project),
			can_update
		])
		.to_string(),
	);
	let state = State::Available {
		connector_id: connector.into(),
		link_id: link.into(),
		review_token: token,
		effective_mode: native.effective_mode.clone(),
		effective_reviewer: native.effective_reviewer.clone(),
		user_mode: native.user_mode.clone(),
		user_reviewer: native.user_reviewer.clone(),
		can_update,
		config_file: native.config_file().into(),
		last_edit: last.as_ref().map(shared::project).map(Box::new),
	};
	if source().await.is_none_or(|after| after.key != before.key) {
		return None;
	}
	Some((before, Review { state, native, guard, prior, scope }))
}
pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F, event: i64) -> State
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	tokio::time::timeout(std::time::Duration::from_secs(40), inspect(store, &source, event))
		.await
		.ok()
		.flatten()
		.map_or(State::Unavailable, |(_, review)| review.state)
}
pub(crate) struct Selection<'a> {
	pub event: i64,
	pub review: &'a str,
	pub edit: &'a ChiefAppSettingEdit,
	pub attempt_id: &'a str,
}
pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	selection: Selection<'_>,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::chief_host::ChiefHostError::Rejected;
	let (before, review) = tokio::time::timeout(
		std::time::Duration::from_secs(40),
		inspect(store, &source, selection.event),
	)
	.await
	.ok()
	.flatten()
	.ok_or(Rejected("Current app settings are unavailable."))?;
	submit(
		store,
		&source,
		before,
		review,
		EditRequest {
			event: Some(selection.event),
			review: selection.review,
			edit: selection.edit,
			attempt_id: selection.attempt_id,
		},
	)
	.await
}
struct EditRequest<'a> {
	event: Option<i64>,
	review: &'a str,
	edit: &'a ChiefAppSettingEdit,
	attempt_id: &'a str,
}
async fn submit<F, Fut>(
	store: &SqliteStore,
	source: &F,
	before: Source,
	review: Review,
	selection: EditRequest<'_>,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::chief_host::ChiefHostError::{Rejected, Unknown};
	let State::Available { can_update: true, review_token, connector_id, link_id, .. } =
		&review.state
	else {
		return Err(Rejected(
			"The native request ended or a shared configuration write is still unconfirmed.",
		));
	};
	if selection.review != review_token {
		return Err(Rejected("The reviewed request or settings changed. Read them again."));
	}
	let (field, value) = selection.edit.native_value();
	let previous_value = shared::raw_app(&review.native, field);
	let value = value.map(|v| json!(v));
	if previous_value == value {
		return Err(Rejected("This override is already saved."));
	}
	let guard = review.guard.ok_or(Rejected("The native request ended."))?;
	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The native source changed."));
	}
	let id = store
		.reserve_chief_app_settings_attempt(ChiefAppSettingsAttempt {
			owner: shared::owner(&before),
			request_event_id: selection.event,
			scope: review.scope,
			connector: connector_id.clone(),
			link: link_id.clone(),
			field: field.into(),
			value,
			previous_value,
			config_version: review.native.config_version().into(),
			review_token: selection.review.into(),
			attempt_id: selection.attempt_id.into(),
			previous_id: review.prior.map(|r| r.id),
		})
		.await
		.map_err(|_| Unknown("The setting reservation is unconfirmed. Refresh saved state."))?
		.ok_or(Rejected("This review was consumed or another shared edit is pending."))?;
	let (_, value) = selection.edit.native_value();
	let native = match selection.edit {
		ChiefAppSettingEdit::ApprovalMode(_) =>
			AppLinkSettingEdit::ApprovalMode(value.map(str::to_owned)),
		ChiefAppSettingEdit::Reviewer(_) => AppLinkSettingEdit::Reviewer(value.map(str::to_owned)),
	};
	let response = if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key)
	{
		Err(ClientError::StaleHistory)
	} else {
		match guard {
			SettingsGuard::Request(g) =>
				before.client.write_app_link_setting_guarded(&review.native, native, g).await,
			SettingsGuard::Saved(g) =>
				before.client.write_saved_app_link_setting(&review.native, native, g).await,
		}
	};
	if source().await.is_some_and(|after| after.key == before.key)
		&& let Err(error) = &response
	{
		crate::native_config_warning::record_settings_error(
			store,
			&before,
			"Settings write or readback failed",
			error,
		)
		.await;
	}
	let (state, version) = match response {
		Ok(ack) => (if ack.overridden { "overridden" } else { "saved" }, Some(ack.version)),
		Err(
			ClientError::StaleHistory
			| ClientError::StaleRequest
			| ClientError::RequestTooLarge
			| ClientError::RequestQueueFull,
		) => ("rejected", None),
		Err(ClientError::Remote(ref e)) if matches!(e.code, -32602..=-32600) => ("rejected", None),
		_ => ("unknown", None),
	};
	// Persist the native ACK before a separate caller read can fail or its source can change.
	if !store
		.finish_chief_app_settings_attempt(id, selection.attempt_id.into(), state.into(), version)
		.await
		.unwrap_or(false)
	{
		return Err(Unknown("The setting result could not be saved. It will not be replayed."));
	}
	match state {
		"saved" | "overridden" => Ok(()),
		"rejected" => Err(Rejected("Native policy or a changed config rejected this edit.")),
		_ => Err(Unknown("The setting write is unconfirmed. It will not be retried.")),
	}
}

fn account_identity(payload: &Value) -> Option<(&str, &str)> {
	if payload["method"] != "mcpServer/elicitation/request"
		|| payload["params"]["serverName"] != "codex_apps"
	{
		return None;
	}
	let meta = &payload["params"]["_meta"];
	let identity = |field| {
		meta[field]
			.as_str()
			.filter(|v| !v.is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control))
	};
	Some((identity("connector_id")?, identity("link_id")?))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn account_selection_requires_native_apps_metadata() {
		let mut value = json!({"method":"mcpServer/elicitation/request","params":{"serverName":"codex_apps",
			"_meta":{"connector_id":"calendar","link_id":" work.一 "},"tool_params":{"link_id":"unrelated"}}});
		assert_eq!(account_identity(&value), Some(("calendar", " work.一 ")));
		value["params"]["_meta"]["link_id"] = Value::Null;
		assert_eq!(account_identity(&value), None);
		value["params"]["_meta"]["link_id"] = json!("work");
		value["params"]["serverName"] = json!("other");
		assert_eq!(account_identity(&value), None);
		value["params"]["serverName"] = json!("codex_apps");
		value["params"]["_meta"]["connector_id"] = json!("bad\nidentity");
		assert_eq!(account_identity(&value), None);
	}
}

#[path = "chief_saved_app_settings.rs"] mod saved;
pub(crate) use saved::{SavedSelection, read_saved, write_saved};
