//! Verify native configured model observations against exact account and task ownership.
use crate::chief_usage_estimate::Source;
use decodex_database::SqliteStore;
use decodex_protocol::ChiefModelSettingsResult as Result;
async fn owned(store: &SqliteStore, source: &Source) -> bool {
	let k = &source.key;
	store
		.chief_thread_is_owned(k.work.clone(), k.thread.clone(), Some(k.generation.as_str().into()))
		.await
		.unwrap_or(false)
}
pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F) -> Result
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return Result::Unavailable };
	if !owned(store, &before).await {
		return Result::Unavailable;
	}
	let Some(guard) = before.client.thread_settings_guard(&before.key.thread) else {
		return Result::Unavailable;
	};
	let response = before.client.thread_model_settings(&before.key.thread, guard.clone()).await;
	if !guard.is_live()
		|| source().await.is_none_or(|after| after.key != before.key)
		|| !owned(store, &before).await
	{
		return Result::Unavailable;
	}
	let settings = match response {
		Ok(Some(settings)) => settings,
		Ok(None) => return Result::NotReported,
		Err(_) => return Result::Unavailable,
	};
	let (
		Ok(work_id),
		Ok(thread_id),
		Ok(account_id),
		Ok(model),
		Ok(reasoning_effort),
		Ok(model_provider),
	) = (
		decodex_protocol::EntityId::new(before.key.work),
		decodex_protocol::EntityId::new(before.key.thread),
		decodex_protocol::EntityId::new(before.key.account.as_str().to_owned()),
		settings.model.map(decodex_protocol::WireText::new).transpose(),
		settings.reasoning_effort.map(decodex_protocol::WireText::new).transpose(),
		settings.model_provider.map(decodex_protocol::WireText::new).transpose(),
	)
	else {
		return Result::Unavailable;
	};
	Result::Available { work_id, thread_id, account_id, model, reasoning_effort, model_provider }
}

#[cfg(test)]
#[path = "chief_model_settings_tests.rs"]
mod tests;
