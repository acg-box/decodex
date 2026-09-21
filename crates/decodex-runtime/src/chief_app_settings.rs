//! Native account configuration readback bound to a pending request and current source.
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::AppLinkSettings;
use decodex_database::SqliteStore;
use decodex_protocol::ChiefAppSettingsResult;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

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
	result.ok().flatten().unwrap_or(ChiefAppSettingsResult::Unavailable)
}

async fn inspect(
	store: &SqliteStore,
	source: &Source,
	event_id: i64,
) -> Option<ChiefAppSettingsResult> {
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
	let owner = store.get_chief_work_item(source.key.work.clone()).await.ok()?;
	if thread == source.key.thread
		&& !params["turnId"].is_null()
		&& (params["turnId"].as_str() != owner.active_turn_id.as_deref()
			|| owner.dispatch_state != decodex_database::ChiefDispatchState::Running)
	{
		return None;
	}
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
	Some(project(source, event_id, connector, link, settings))
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
	settings: AppLinkSettings,
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
		effective_mode: settings.effective_mode,
		effective_reviewer: settings.effective_reviewer,
		user_mode: settings.user_mode,
		user_reviewer: settings.user_reviewer,
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
