//! Explicit sample commands use the current admitted native connection without blocking its actor.
use std::time::Duration;

use tokio::{sync::oneshot::Receiver, time};

use crate::{
	agent_host::{AgentHost, AgentHostError},
	agent_recap,
	agent_usage_estimate::Source,
};
use decodex_codex::app_server_client::{ClientError, NativeThreadSampleOutcome};
use decodex_database::AgentVoiceHistoryRevision;
use decodex_protocol::{
	AgentActionDto, AgentSampleAnswer, AgentSampleStatus, EntityId, MAX_AGENT_SAMPLE_PROMPT_BYTES,
	WireText,
};

impl AgentHost {
	pub(super) async fn handle_sample(
		&self,
		key: &str,
		action: AgentActionDto,
	) -> Result<String, AgentHostError> {
		match action {
			AgentActionDto::GenerateSample { work_id, thread_id, prompt } =>
				self.start_sample(key, work_id.as_str(), thread_id.as_str(), prompt.as_str().into())
					.await,
			AgentActionDto::CancelSample { work_id, request_id } => {
				self.samples.cancel_request(work_id.as_str(), request_id.as_str())?;

				Ok(work_id.as_str().to_owned())
			},
			_ => unreachable!("sample action"),
		}
	}

	async fn sample_source(&self, work: &str, thread: &str) -> Option<Source> {
		let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

		if owner.parent_goal_id.is_some() || owner.codex_thread_id.as_deref() != Some(thread) {
			return None;
		}

		let source = self.timeline_source(work, thread).await?;

		self.store
			.agent_thread_is_owned(
				work.into(),
				thread.into(),
				Some(source.key.generation.as_str().into()),
			)
			.await
			.ok()?
			.then_some(source)
	}

	pub(crate) async fn sample_status(&self, work: EntityId) -> AgentSampleStatus {
		let owner = self.store.get_agent_work_item(work.as_str().into()).await.ok();
		let source = match owner.and_then(|v| v.codex_thread_id) {
			Some(thread) => self.sample_source(work.as_str(), &thread).await,
			None => None,
		};
		let voice = match &source {
			Some(s) => self
				.store
				.agent_voice_history_revision(work.as_str().into(), s.key.thread.clone())
				.await
				.ok(),
			None => None,
		};

		self.samples.status(work, source.as_ref(), voice.as_ref())
	}

	pub(super) async fn start_sample(
		&self,
		key: &str,
		work: &str,
		thread: &str,
		prompt: String,
	) -> Result<String, AgentHostError> {
		if prompt.trim().is_empty() || prompt.len() > MAX_AGENT_SAMPLE_PROMPT_BYTES {
			return Err("Enter a question of at most 4000 UTF-8 bytes".into());
		}

		let source = self
			.sample_source(work, thread)
			.await
			.ok_or("Sampling requires a connected root conversation")?;
		let voice = self
			.store
			.agent_voice_history_revision(work.into(), thread.into())
			.await
			.map_err(|_| "Voice history is unavailable")?;

		if voice.open_calls > 0 {
			return Err("Finish the voice conversation before asking separately".into());
		}

		let cancellation = self.samples.start(
			Source { key: source.key.clone(), client: source.client.clone() },
			key,
			&prompt,
			voice.clone(),
		)?;
		let host = self.clone();
		let key = key.to_owned();

		tokio::spawn(async move {
			host.run_sample(source, key, prompt, voice, cancellation).await;
		});

		Ok(work.into())
	}

	async fn run_sample(
		&self,
		source: Source,
		key: String,
		prompt: String,
		voice: AgentVoiceHistoryRevision,
		cancellation: Receiver<()>,
	) {
		let result = time::timeout(Duration::from_secs(40), async {
			let guard = source
				.client
				.history_guard(source.key.history_revision)
				.ok_or(ClientError::StaleHistory)?;
			let latest = source
				.client
				.thread_latest_turn_id(&source.key.thread)
				.await?
				.ok_or(ClientError::InvalidFrame)?;
			let native = source
				.client
				.sample_thread_cancellable(
					&source.key.thread,
					&latest,
					&prompt,
					guard,
					cancellation,
				)
				.await?;
			let NativeThreadSampleOutcome::Answer(native) = native else { return Ok(None) };
			let current = self
				.sample_source(&source.key.work, &source.key.thread)
				.await
				.ok_or(ClientError::Closed)?;

			if !agent_recap::same_owner(&source, &current)
				|| self
					.store
					.agent_voice_history_revision(
						source.key.work.clone(),
						source.key.thread.clone(),
					)
					.await
					.ok()
					.as_ref()
					!= Some(&voice)
			{
				return Err(ClientError::StaleHistory);
			}

			Ok(Some(AgentSampleAnswer {
				source_turn_id: WireText::new(native.source_turn_id)
					.map_err(|_| ClientError::InvalidFrame)?,
				response_id: WireText::new(native.response_id)
					.map_err(|_| ClientError::InvalidFrame)?,
				text: native.text.filter(|text| !text.trim().is_empty()),
				input_tokens: native.input_tokens,
				cached_input_tokens: native.cached_input_tokens,
				output_tokens: native.output_tokens,
			}))
		})
		.await
		.unwrap_or(Err(ClientError::Io));

		self.samples.finish(&source, &key, result);
	}
}
