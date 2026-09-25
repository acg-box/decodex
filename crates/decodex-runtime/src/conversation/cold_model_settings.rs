//! Inspect persisted native settings through the existing bounded metadata process owner.
use super::{
	super::{
		ConversationId, ConversationRefreshCallback, ConversationRuntime,
		ProcessAccountRefreshCallback, ProcessGenerationId, derived_uuid,
	},
	ResultDto, project,
};
use std::{sync::Arc, time::Duration};

impl ConversationRuntime {
	pub(super) async fn cold_model_settings(&self, key: &str, conversation: &str) -> ResultDto {
		let Ok(permit) = self.inner.initial_catalog.clone().try_lock_owned() else {
			return ResultDto::Unavailable;
		};
		let mut workers = self.inner.workers.lock().await;
		if self.is_shutting_down() {
			return ResultDto::Unavailable;
		}
		while workers.try_join_next().is_some() {}
		let runtime = self.clone();
		let key = key.to_owned();
		let conversation = conversation.to_owned();
		let (reply, result) = tokio::sync::oneshot::channel();
		workers.spawn(async move {
			let _permit = permit;
			let observed = runtime.read_cold_model_settings(&key, &conversation).await;
			let _ = reply.send(observed.unwrap_or(ResultDto::Unavailable));
		});
		drop(workers);
		tokio::time::timeout(Duration::from_secs(35), result)
			.await
			.ok()
			.and_then(Result::ok)
			.unwrap_or(ResultDto::Unavailable)
	}

	async fn read_cold_model_settings(&self, key: &str, conversation: &str) -> Option<ResultDto> {
		let id = ConversationId::new(conversation).ok()?;
		if self.local().contains_key(conversation) {
			return None;
		}
		let source =
			self.inner.store.read_ordinary_runtime_session_for_resume(&id).await.ok()??;
		let request = self.inner.store.read_conversation_request(&id).await.ok()??;
		let account = source.source_account_id.clone();
		let revision = self.inner.accounts.inspect(&account).await.ok()?.account.revision;
		let credential = tokio::time::timeout(
			Duration::from_secs(10),
			self.inner.accounts.process_credential(&account, revision),
		)
		.await
		.ok()?
		.ok()?;
		let callback: Arc<dyn ProcessAccountRefreshCallback> =
			Arc::new(ConversationRefreshCallback {
				accounts: self.inner.accounts.clone(),
				runtime: tokio::runtime::Handle::current(),
				generation_id: ProcessGenerationId::new(derived_uuid(
					"cold-model-settings-process",
					&[key, account.as_str()],
				))
				.ok()?,
			});
		let runtime = self.clone();
		let read_account = account.clone();
		let directory = request.working_directory.clone();
		let thread = source.codex_thread_id.clone();
		let settings = tokio::task::spawn_blocking(move || {
			runtime.read_metadata_process(
				&read_account,
				revision,
				&directory,
				credential,
				callback,
				|child| {
					let (settings, events) = child.read_ordinary_model_settings(&thread);
					child.retain_ordinary_events(events).ok()?;
					settings.ok().flatten()
				},
			)
		})
		.await
		.ok()??;
		if self.is_shutting_down()
			|| !self.inner.store.account_is_ready_at_revision(&account, revision).await.ok()?
			|| self.inner.store.read_ordinary_runtime_session_for_resume(&id).await.ok()?.as_ref()
				!= Some(&source)
			|| self.inner.store.read_conversation_request(&id).await.ok()?.as_ref()
				!= Some(&request)
		{
			return None;
		}
		if self.local().contains_key(conversation) {
			return None;
		}
		// Neither the initial request nor thread/read proves the last requested tier.
		project(settings, None)
	}
}
