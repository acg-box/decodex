//! Verify native configured model observations against exact account and task ownership.
use std::future::Future;

use crate::agent_usage_estimate::Source;
use decodex_database::SqliteStore;
use decodex_protocol::{EntityId, WireText};

pub(crate) async fn read<F, Fut>(
	store: &SqliteStore,
	source: F,
) -> decodex_protocol::AgentModelSettingsResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return decodex_protocol::AgentModelSettingsResult::Unavailable;
	};

	if !readable(store, &before).await {
		return decodex_protocol::AgentModelSettingsResult::Unavailable;
	}

	let Some(guard) = before.client.thread_settings_guard(&before.key.thread) else {
		return decodex_protocol::AgentModelSettingsResult::Unavailable;
	};
	let response = before.client.thread_model_settings(&before.key.thread, guard.clone()).await;
	let mismatch = before.client.refresh_provider_mismatch(&before.key.thread).await;

	if !guard.is_live()
		|| source().await.is_none_or(|after| after.key != before.key)
		|| !readable(store, &before).await
	{
		return decodex_protocol::AgentModelSettingsResult::Unavailable;
	}
	let Ok(mismatch) = mismatch else {
		return decodex_protocol::AgentModelSettingsResult::Unavailable;
	};
	if let Some(mismatch) = mismatch {
		let (
			Ok(work_id),
			Ok(thread_id),
			Ok(account_id),
			Ok(model_provider),
			Ok(current_model_provider),
		) = (
			EntityId::new(before.key.work),
			EntityId::new(before.key.thread),
			EntityId::new(before.key.account.as_str().to_owned()),
			WireText::new(mismatch.model_provider),
			WireText::new(mismatch.current_model_provider),
		)
		else {
			return decodex_protocol::AgentModelSettingsResult::Unavailable;
		};
		return decodex_protocol::AgentModelSettingsResult::ProviderMismatch {
			work_id,
			thread_id,
			account_id,
			model_provider,
			current_model_provider,
		};
	}

	let settings = match response {
		Ok(Some(settings)) => settings,
		Ok(None) => return decodex_protocol::AgentModelSettingsResult::NotReported,
		Err(_) => return decodex_protocol::AgentModelSettingsResult::Unavailable,
	};
	let service_tier = settings.service_tier.as_ref().and_then(|s| WireText::new(s.clone()).ok());
	let (
		Ok(work_id),
		Ok(thread_id),
		Ok(account_id),
		Ok(model),
		Ok(reasoning_effort),
		Ok(model_provider),
	) = (
		EntityId::new(before.key.work),
		EntityId::new(before.key.thread),
		EntityId::new(before.key.account.as_str().to_owned()),
		settings.model.map(WireText::new).transpose(),
		settings.reasoning_effort.map(WireText::new).transpose(),
		settings.model_provider.map(WireText::new).transpose(),
	)
	else {
		return decodex_protocol::AgentModelSettingsResult::Unavailable;
	};

	decodex_protocol::AgentModelSettingsResult::Available {
		work_id,
		thread_id,
		account_id,
		model,
		reasoning_effort,
		model_provider,
		service_tier,
	}
}

async fn readable(store: &SqliteStore, source: &Source) -> bool {
	let k = &source.key;

	// Like history and archive inspection, this reads the shared native catalog
	// without adopting a saved conversation's execution process.
	store.account_is_ready_at_revision(&k.account, k.revision).await.unwrap_or(false)
		&& store
			.get_agent_work_item(k.work.clone())
			.await
			.is_ok_and(|work| work.codex_thread_id.as_deref() == Some(k.thread.as_str()))
}

#[cfg(test)]
#[path = "agent_model_settings_tests.rs"]
pub(crate) mod tests;
