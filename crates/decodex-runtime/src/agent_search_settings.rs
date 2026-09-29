//! Keep search configuration observations bound to the current native owner.
use crate::{agent_host::AgentHostError, agent_usage_estimate::Source};
use decodex_codex::app_server_client::NativeSearchSettings;
use decodex_protocol::{AgentSearchSettingsResult, EntityId, WireText};
use serde_json::json;
use sha2::{Digest as _, Sha256};

async fn inspect(source: &Source) -> Option<(NativeSearchSettings, String)> {
	let native = source.client.thread_read(json!({"threadId":source.key.thread})).await.ok()?;
	if native["thread"]["id"] != source.key.thread {
		return None;
	}
	let settings = source.client.search_settings(native["thread"]["cwd"].as_str()?).await.ok()?;
	let key = &source.key;
	let identity = json!([
		key.work,
		key.thread,
		key.generation.as_str(),
		key.account.as_str(),
		key.revision,
		key.history_revision,
		source.client.connection_identity(),
		settings.fingerprint()
	]);
	let token = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	Some((settings, token))
}

pub(crate) async fn read<F, Fut>(source: F) -> AgentSearchSettingsResult
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return AgentSearchSettingsResult::Unavailable };
	let result = tokio::time::timeout(std::time::Duration::from_secs(25), inspect(&before)).await;
	if source().await.is_none_or(|after| {
		after.key != before.key
			|| after.client.connection_identity() != before.client.connection_identity()
	}) {
		return AgentSearchSettingsResult::Unavailable;
	}
	let Some((settings, token)) = result.ok().flatten() else {
		return AgentSearchSettingsResult::Unavailable;
	};
	project(&before.key.work, settings, token).unwrap_or(AgentSearchSettingsResult::Unavailable)
}

fn project(
	work: &str,
	settings: NativeSearchSettings,
	token: String,
) -> Option<AgentSearchSettingsResult> {
	let result = AgentSearchSettingsResult::Available {
		work_id: EntityId::new(work).ok()?,
		review_token: WireText::new(token).ok()?,
		modes: settings.modes.into_iter().map(WireText::new).collect::<Result<_, _>>().ok()?,
		effective: settings.effective.map(WireText::new).transpose().ok()?,
		preference: settings.preference.map(WireText::new).transpose().ok()?,
	};
	(serde_json::to_vec(&result).ok()?.len() <= 32 * 1024).then_some(result)
}

pub(crate) async fn write<F, Fut>(source: F, review: &str, mode: &str) -> Result<(), AgentHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use AgentHostError::{Rejected, Unknown};
	let before = source().await.ok_or(Rejected("Search settings are unavailable."))?;
	let (settings, token) =
		tokio::time::timeout(std::time::Duration::from_secs(25), inspect(&before))
			.await
			.ok()
			.flatten()
			.ok_or(Rejected("Refresh search settings before choosing a mode."))?;
	if token != review
		|| source().await.is_none_or(|after| {
			after.key != before.key
				|| after.client.connection_identity() != before.client.connection_identity()
		})
		|| !settings.modes.iter().any(|v| v == mode)
	{
		return Err(Rejected("Search settings changed. Refresh the search modes."));
	}
	let saved = before.client.write_search_mode(&settings, mode).await.map_err(|_| {
		Unknown("Search save or readback is unconfirmed. Refresh settings before trying again.")
	})?;
	if source().await.is_none_or(|after| {
		after.key != before.key
			|| after.client.connection_identity() != before.client.connection_identity()
	}) || saved.preference.as_deref() != Some(mode)
	{
		return Err(Unknown("Search save is unconfirmed for this task. Refresh settings."));
	}
	// A project override is a valid saved preference; the fresh query explains the effective value.
	Ok(())
}

#[cfg(test)]
#[path = "agent_search_settings_tests.rs"]
mod tests;
