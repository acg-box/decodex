//! Native account configuration readback bound to a pending request and current source.
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::{AppLinkSettingEdit, AppLinkSettings, ServerRequestGuard};
use decodex_database::SqliteStore;
use decodex_protocol::ChiefAppSettingsResult;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

struct Inspection {
	state: ChiefAppSettingsResult,
	settings: AppLinkSettings,
	guard: ServerRequestGuard,
}

pub(crate) async fn read<F, Fut>(
	store: &SqliteStore,
	source: F,
	event: i64,
) -> ChiefAppSettingsResult
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return ChiefAppSettingsResult::Unavailable };
	let result =
		tokio::time::timeout(std::time::Duration::from_secs(40), inspect(store, &before, event))
			.await;
	if source().await.is_none_or(|after| after.key != before.key) {
		return ChiefAppSettingsResult::Unavailable;
	}
	result.ok().flatten().map_or(ChiefAppSettingsResult::Unavailable, |value| value.state)
}

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	event: i64,
	review: &str,
	edit: &decodex_protocol::ChiefAppSettingEdit,
	attempt: &str,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::chief_host::ChiefHostError::{Rejected, Unknown};
	let before = source().await.ok_or(Rejected("Account settings source is unavailable."))?;
	let inspected =
		tokio::time::timeout(std::time::Duration::from_secs(40), inspect(store, &before, event))
			.await
			.ok()
			.flatten()
			.ok_or(Rejected(
				"Refresh account settings; the native request is no longer available.",
			))?;
	let ChiefAppSettingsResult::Available { connector_id, link_id, review_token, .. } =
		&inspected.state
	else {
		return Err(Rejected("Account settings are unavailable."));
	};
	if review_token != review || source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("Account settings or their source changed. Review them again."));
	}
	let (field, value) = edit.native_value();
	let reserved = store
		.reserve_chief_app_settings_attempt(decodex_database::ChiefAppSettingsAttempt {
			event_id: event,
			work_id: before.key.work.clone(),
			thread_id: before.key.thread.clone(),
			generation_id: Some(before.key.generation.as_str().into()),
			connector_id: connector_id.clone(),
			link_id: link_id.clone(),
			review_token: review.into(),
			field: field.into(),
			value: value.map(str::to_owned),
			attempt_id: attempt.into(),
		})
		.await
		.map_err(|_| {
			Unknown(
				"Setting dispatch could not be reserved. Read current settings before any further action.",
			)
		})?;
	if !reserved {
		return Err(Unknown(
			"This reviewed edit was already submitted or reserved. Read current settings; it will not be sent again.",
		));
	}
	if source().await.is_none_or(|after| after.key != before.key) || !inspected.guard.is_live() {
		return Err(Rejected(
			"The account or native request changed before dispatch. Refresh settings.",
		));
	}
	let native = match edit {
		decodex_protocol::ChiefAppSettingEdit::ApprovalMode(_) =>
			AppLinkSettingEdit::ApprovalMode(value.map(str::to_owned)),
		decodex_protocol::ChiefAppSettingEdit::Reviewer(_) =>
			AppLinkSettingEdit::Reviewer(value.map(str::to_owned)),
	};
	let result = before
		.client
		.write_app_link_setting_guarded(&inspected.settings, native, inspected.guard)
		.await
		.map_err(|_| {
			Unknown(
				"Setting write or readback is unconfirmed. Refresh settings; do not automatically retry.",
			)
		})?;
	if source().await.is_none_or(|after| after.key != before.key) {
		return Err(Unknown(
			"The setting may have been saved, but its source changed. Read settings on the original account.",
		));
	}
	let observed = if field == "approvals_reviewer" {
		result.settings.user_reviewer.as_deref()
	} else {
		result.settings.user_mode.as_deref()
	};
	if observed != value {
		return Err(Unknown(
			"Native readback differs from the requested setting. Refresh and review current configuration.",
		));
	}
	// A saved override does not prove that every loaded thread or tool uses it.
	Ok(())
}

async fn inspect(store: &SqliteStore, source: &Source, event_id: i64) -> Option<Inspection> {
	let event = store.get_chief_inbox_event(event_id).await.ok()?;
	if event.work_item_id != source.key.work
		|| event.disposition.is_some()
		|| event.event_kind != "server_request_pending"
	{
		return None;
	}
	if !store
		.chief_thread_is_owned(
			source.key.work.clone(),
			source.key.thread.clone(),
			Some(source.key.generation.as_str().into()),
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
	// A yielded native tool can request approval in a later turn while retaining
	// its originating turn ID. The exact transport request below owns liveness.
	if thread != source.key.thread {
		let owner = crate::chief::native_subagents::request_owner(store, &source.client, thread)
			.await
			.ok()?;
		if owner.id != source.key.work
			|| owner.codex_thread_id.as_deref() != Some(&source.key.thread)
			|| payload["ownerThreadId"].as_str() != Some(&source.key.thread)
		{
			return None;
		}
	}
	let request_id = serde_json::from_value(payload["id"].clone()).ok()?;
	let guard =
		source.client.server_request_guard(&request_id, "mcpServer/elicitation/request", params)?;
	let native = source.client.thread_read(json!({"threadId":thread})).await.ok()?;
	if native["thread"]["id"] != thread {
		return None;
	}
	let settings = source
		.client
		.app_link_settings(native["thread"]["cwd"].as_str()?, connector, link)
		.await
		.ok()?;
	if !guard.is_live() || store.get_chief_inbox_event(event_id).await.ok()?.disposition.is_some() {
		return None;
	}
	Some(Inspection {
		state: project(source, event_id, connector, link, &settings),
		settings,
		guard,
	})
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

fn project(
	source: &Source,
	event: i64,
	connector: &str,
	link: &str,
	settings: &AppLinkSettings,
) -> ChiefAppSettingsResult {
	let key = &source.key;
	let facts = json!([
		key.generation.as_str(),
		key.account.as_str(),
		key.revision,
		key.history_revision,
		key.thread,
		key.work,
		event,
		settings.review_fingerprint()
	]);
	ChiefAppSettingsResult::Available {
		connector_id: connector.into(),
		link_id: link.into(),
		review_token: Sha256::digest(facts.to_string().as_bytes())
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect(),
		effective_mode: settings.effective_mode.clone(),
		effective_reviewer: settings.effective_reviewer.clone(),
		user_mode: settings.user_mode.clone(),
		user_reviewer: settings.user_reviewer.clone(),
	}
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
