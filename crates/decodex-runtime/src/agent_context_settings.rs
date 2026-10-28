//! Keep context configuration observations bound to the current native owner.
use std::{future::Future, time::Duration};

use AgentHostError::{Rejected, Unknown};
use sha2::{Digest as _, Sha256};
use tokio::time;

use crate::{agent_host::AgentHostError, agent_usage_estimate::Source};
use decodex_codex::app_server_client::{CONTEXT_FEATURES, NativeContextSettings};
use decodex_protocol::{
	AgentContextFeature, AgentContextPreference, AgentContextSettingsResult, EntityId, WireText,
};

pub(crate) async fn read<F, Fut>(source: F) -> AgentContextSettingsResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return AgentContextSettingsResult::Unavailable };
	let result = time::timeout(Duration::from_secs(25), inspect(&before)).await;

	if source().await.is_none_or(|after| {
		after.key != before.key
			|| after.client.connection_identity() != before.client.connection_identity()
	}) {
		return AgentContextSettingsResult::Unavailable;
	}

	let Some((settings, token)) = result.ok().flatten() else {
		return AgentContextSettingsResult::Unavailable;
	};

	project(&before.key.work, settings, token).unwrap_or(AgentContextSettingsResult::Unavailable)
}

pub(crate) async fn write<F, Fut>(
	source: F,
	review: &str,
	feature: AgentContextFeature,
	enabled: bool,
) -> Result<(), AgentHostError>
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let before = source().await.ok_or(Rejected("Context settings are unavailable."))?;
	let (settings, token) = time::timeout(Duration::from_secs(25), inspect(&before))
		.await
		.ok()
		.flatten()
		.ok_or(Rejected("Refresh context settings before changing a preference."))?;

	if token != review
		|| source().await.is_none_or(|after| {
			after.key != before.key
				|| after.client.connection_identity() != before.client.connection_identity()
		}) {
		return Err(Rejected("Context settings changed. Refresh the context preferences."));
	}

	let result = match &feature {
		AgentContextFeature::AsyncCompaction =>
			before.client.write_context_preference(&settings, CONTEXT_FEATURES[0], enabled).await,
		AgentContextFeature::RetainClientToolOutputs =>
			before.client.write_context_preference(&settings, CONTEXT_FEATURES[1], enabled).await,
		AgentContextFeature::McpCatalogStartup { server } =>
			before.client.write_mcp_startup_preference(&settings, server.as_str(), enabled).await,
	};
	let saved = result.map_err(|_| {
		Unknown("Context save or readback is unconfirmed. Refresh settings before trying again.")
	})?;

	if source().await.is_none_or(|after| {
		after.key != before.key
			|| after.client.connection_identity() != before.client.connection_identity()
	}) || preference(&saved, &feature) != Some(enabled)
	{
		return Err(Unknown("Context save is unconfirmed for this task. Refresh settings."));
	}

	// A project override is a valid saved preference; the fresh query explains the effective value.
	Ok(())
}

fn project(
	work: &str,
	settings: NativeContextSettings,
	token: String,
) -> Option<AgentContextSettingsResult> {
	let mut preferences: Vec<_> =
		[AgentContextFeature::AsyncCompaction, AgentContextFeature::RetainClientToolOutputs]
			.into_iter()
			.enumerate()
			.map(|(index, feature)| AgentContextPreference {
				feature,
				effective: settings.effective[index],
				preference: settings.preference[index],
			})
			.collect();

	for (server, effective, preference) in settings.mcp_startup {
		preferences.push(AgentContextPreference {
			feature: AgentContextFeature::McpCatalogStartup { server: WireText::new(server).ok()? },
			effective,
			preference,
		});
	}

	let result = AgentContextSettingsResult::Available {
		work_id: EntityId::new(work).ok()?,
		review_token: WireText::new(token).ok()?,
		preferences,
	};

	(serde_json::to_vec(&result).ok()?.len() <= 32 * 1_024).then_some(result)
}

fn preference(settings: &NativeContextSettings, feature: &AgentContextFeature) -> Option<bool> {
	match feature {
		AgentContextFeature::AsyncCompaction => settings.preference[0],
		AgentContextFeature::RetainClientToolOutputs => settings.preference[1],
		AgentContextFeature::McpCatalogStartup { server } => settings
			.mcp_startup
			.iter()
			.find(|(name, _, _)| name == server.as_str())
			.and_then(|(_, _, saved)| *saved),
	}
}

async fn inspect(source: &Source) -> Option<(NativeContextSettings, String)> {
	let native =
		source.client.thread_read(serde_json::json!({"threadId":source.key.thread})).await.ok()?;

	if native["thread"]["id"] != source.key.thread {
		return None;
	}

	let settings = source.client.context_settings(native["thread"]["cwd"].as_str()?).await.ok()?;
	let key = &source.key;
	let identity = serde_json::json!([
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

#[cfg(test)]
#[path = "agent_context_settings_tests.rs"]
mod tests;
