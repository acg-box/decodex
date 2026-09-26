//! Start recap work through the existing Chief command owner without blocking its event loop.
use crate::{
	chief_host::{ChiefHost, ChiefHostError},
	chief_usage_estimate::Source,
};
use decodex_protocol::{EntityId, TaskRecapStatus};
use tokio::sync::watch;

impl ChiefHost {
	pub(super) async fn handle_recap(
		&self,
		key: &str,
		action: decodex_protocol::ChiefActionDto,
	) -> Result<String, ChiefHostError> {
		match action {
			decodex_protocol::ChiefActionDto::GenerateRecap { work_id, thread_id } =>
				self.start_recap(key, work_id.as_str(), thread_id.as_str()).await,
			decodex_protocol::ChiefActionDto::CancelRecap { work_id, request_id } => {
				self.recaps.cancel_request(work_id.as_str(), request_id.as_str());
				Ok(work_id.as_str().into())
			},
			_ => Err(ChiefHostError::Rejected("Unsupported recap command")),
		}
	}

	async fn recap_source(&self, work: &str, thread: &str) -> Option<Source> {
		let source = self.timeline_source(work, thread).await?;
		self.store
			.chief_thread_is_owned(
				work.into(),
				thread.into(),
				Some(source.key.generation.as_str().into()),
			)
			.await
			.ok()?
			.then_some(source)
	}

	pub(crate) async fn recap_status(&self, work: EntityId) -> TaskRecapStatus {
		let source = match self
			.store
			.get_chief_work_item(work.as_str().into())
			.await
			.ok()
			.and_then(|v| v.codex_thread_id)
		{
			Some(thread) => self.recap_source(work.as_str(), &thread).await,
			None => None,
		};
		let mut result = self.recaps.status(work.clone(), source.as_ref());
		if result.phase != decodex_protocol::TaskRecapPhase::Idle {
			let revision = match &source {
				Some(source) => self
					.store
					.chief_voice_history_revision(work.as_str().into(), source.key.thread.clone())
					.await
					.ok(),
				None => None,
			};
			if revision.as_ref().is_none_or(|v| !self.recaps.voice_is_current(work.as_str(), v)) {
				result.phase = decodex_protocol::TaskRecapPhase::Cancelled;
				result.recap = None;
			}
		}
		result
	}

	pub(super) async fn start_recap(
		&self,
		key: &str,
		work: &str,
		thread: &str,
	) -> Result<String, ChiefHostError> {
		let source =
			self.recap_source(work, thread).await.ok_or("Task connection is unavailable")?;
		let voice = self
			.store
			.read_chief_voice_history(work.into(), thread.into())
			.await
			.map_err(|_| "Voice history is unavailable")?;
		if voice.revision.open_calls > 0 {
			return Err("Finish the voice conversation before generating a recap".into());
		}
		let copy = Source { key: source.key.clone(), client: source.client.clone() };
		let cancelled = self.recaps.start(copy, key, voice.revision.clone())?;
		let host = self.clone();
		let key = key.to_owned();
		tokio::spawn(async move {
			host.run_recap(key, source, cancelled, voice).await;
		});
		Ok(work.into())
	}

	async fn recap_owner_is_current(
		&self,
		source: &Source,
		voice: &decodex_database::ChiefVoiceHistoryRevision,
	) -> bool {
		if self
			.store
			.chief_voice_history_revision(source.key.work.clone(), source.key.thread.clone())
			.await
			.ok()
			.as_ref() != Some(voice)
		{
			return false;
		}
		self.recap_source(&source.key.work, &source.key.thread)
			.await
			.is_some_and(|current| crate::chief_recap::same_owner(source, &current))
	}

	async fn run_recap(
		&self,
		key: String,
		source: Source,
		cancelled: watch::Receiver<bool>,
		voice: decodex_database::ChiefVoiceHistory,
	) {
		let mut temporary_id = None;
		let result = async {
			let prepared = crate::chief_recap::prepare(&source, Some(&voice)).await?;
			if *cancelled.borrow()
				|| !prepared.guard.is_live()
				|| !self.recap_owner_is_current(&source, &voice.revision).await
			{
				return None;
			}
			let temporary =
				source.client.start_temporary_structured(prepared.options).await.ok()?;
			temporary_id = Some(temporary.id().to_owned());
			let latest = source.client.thread_latest_turn_id(&source.key.thread).await;
			if *cancelled.borrow()
				|| !prepared.guard.is_live()
				|| !self.recap_owner_is_current(&source, &voice.revision).await
				|| latest.ok() != Some(prepared.latest_turn)
			{
				let _ = temporary.cancel().await;
				return None;
			}
			let Some(events) = self.recaps.register(temporary.id()) else {
				let _ = temporary.cancel().await;
				return None;
			};
			let output = temporary
				.run(prepared.prompt, crate::chief_recap::schema(), None, events, cancelled.clone())
				.await
				.ok()?;
			if *cancelled.borrow()
				|| !prepared.guard.is_live()
				|| !self.recap_owner_is_current(&source, &voice.revision).await
			{
				return None;
			}
			crate::chief_recap::parse(&output)
		}
		.await;
		self.recaps.finish(&source.key.work, &key, temporary_id.as_deref(), result);
	}
}
