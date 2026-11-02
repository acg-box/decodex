//! Run title reviews off the command actor and fence publication to their originating source.
use crate::{
	agent_host::AgentHost, agent_message_history, agent_titles, agent_usage_estimate::Source,
};
use decodex_codex::app_server_client::{
	HistoryGuard, NativeThreadModelSettings, TemporaryStructuredOptions,
};
use decodex_database::AgentConversationTitle;
use decodex_protocol::AgentCapabilitiesResult;
use tokio::sync::watch;

impl AgentHost {
	pub(super) fn schedule_title(&self, work: String, thread: String, turn: String) {
		let Some(cancel) = self.titles.start(&work, &thread, &turn) else { return };
		let host = self.clone();
		tokio::spawn(async move {
			let _ = host.review_title(&work, &thread, &turn, cancel).await;
			host.titles.finish(&work, &thread, &turn);
		});
	}

	async fn title_owner_is_current(&self, source: &Source, guard: &HistoryGuard) -> bool {
		guard.is_live()
			&& self
				.owned_timeline_source(&source.key.work, &source.key.thread)
				.await
				.is_some_and(|current| crate::agent_usage_estimate::same_owner(source, &current))
	}

	async fn review_title(
		&self,
		work: &str,
		thread: &str,
		turn: &str,
		cancel: watch::Receiver<bool>,
	) -> Option<()> {
		let source = self.owned_timeline_source(work, thread).await?;
		let review =
			self.store.agent_title_review(work.into(), thread.into(), turn.into()).await.ok()??;
		let metadata = source
			.client
			.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false}))
			.await
			.ok()?;
		if metadata["thread"]["id"] != thread {
			return None;
		}
		let name = metadata["thread"]["name"].as_str().map(str::to_owned);
		let item = self.store.get_agent_work_item(work.into()).await.ok()?;
		let mut state = review.state.unwrap_or_else(|| AgentConversationTitle {
			automatic: name.as_deref().is_none_or(|n| {
				n == item.title
					|| matches!(
						n,
						"Assistant" | "New Agent" | "Agent" | "Main" | "New conversation"
					)
			}),
			generated: false,
			name: name.clone(),
			reviewed_through: 0,
		});
		if state.name != name || !state.automatic {
			state.name = name;
			state.automatic = false;
			state.generated = true;
			state.reviewed_through = review.completed_event;
			self.save_title(&source, state).await?;
			return Some(());
		}
		if state.reviewed_through >= review.completed_event
			|| (state.generated && review.new_exchanges < 4)
		{
			return Some(());
		}
		let (permissions, guard) = source.client.configured_task_permissions(thread)?;
		let guard = source.client.with_thread_name_guard(thread, guard)?;
		let model = NativeThreadModelSettings::from_read_response(metadata, thread).ok()??;
		let history = agent_message_history::read_for_title(&source.client, thread).await.ok()?;
		if history.latest_turn.as_deref() != Some(turn)
			|| history.exchanges.iter().all(|e| e.user.is_empty())
		{
			return None;
		}
		let prompt = agent_titles::prompt(
			state.generated.then_some(state.name.as_deref()).flatten(),
			&history,
		);
		let model = title_model(&source, model).await?;
		if *cancel.borrow() || !self.title_owner_is_current(&source, &guard).await {
			return None;
		}
		// Advance the checkpoint only after a successful review and native name readback.
		// A failed or cancelled review keeps the last title and can retry on the next completion.
		state.reviewed_through = review.completed_event;
		let temporary = source
			.client
			.start_temporary_structured(TemporaryStructuredOptions {
				model: model.name,
				model_provider: model.provider,
				cwd: permissions.cwd,
				active_permission_profile: permissions.profile_id,
				mcp_server_names: Vec::new(),
			})
			.await
			.ok()?;
		if *cancel.borrow() || !self.title_owner_is_current(&source, &guard).await {
			let _ = temporary.cancel().await;
			return None;
		}
		let output = temporary
			.run_isolated(prompt, agent_titles::schema(), model.effort, cancel.clone())
			.await
			.ok()?;
		let candidate = agent_titles::parse(&output)?;
		if *cancel.borrow() || !self.title_owner_is_current(&source, &guard).await {
			return None;
		}
		self.publish_title(&source, &guard, state, candidate).await
	}

	async fn save_title(&self, source: &Source, state: AgentConversationTitle) -> Option<()> {
		self.store
			.observe_agent_title(
				source.key.work.clone(),
				source.key.thread.clone(),
				Some(source.key.generation.as_str().into()),
				state,
				false,
			)
			.await
			.ok()?
			.then_some(())
	}

	async fn publish_title(
		&self,
		source: &Source,
		guard: &HistoryGuard,
		mut state: AgentConversationTitle,
		candidate: String,
	) -> Option<()> {
		let read = source
			.client
			.thread_read(serde_json::json!({"threadId":source.key.thread,"includeTurns":false}))
			.await
			.ok()?;
		if read["thread"]["id"] != source.key.thread
			|| !self.title_owner_is_current(source, guard).await
		{
			return None;
		}
		let current = read["thread"]["name"].as_str().map(str::to_owned);
		if current != state.name {
			state.name = current;
			state.automatic = false;
			state.generated = true;
			return self.save_title(source, state).await;
		}
		if current.as_deref() != Some(&candidate) {
			// Native naming has no compare-and-set. Re-read immediately before the guarded write,
			// then reconcile the actual name, including a lost acknowledgement. Never replay it.
			let _ = source
				.client
				.name_thread_guarded(&source.key.thread, &candidate, guard.clone())
				.await;
			let read = source
				.client
				.thread_read(serde_json::json!({"threadId":source.key.thread,"includeTurns":false}))
				.await
				.ok()?;
			if read["thread"]["id"] != source.key.thread
				|| read["thread"]["name"].as_str() != Some(&candidate)
			{
				return None;
			}
		}
		state.name = Some(candidate);
		state.generated = true;
		self.save_title(source, state).await
	}
}

struct TitleModel {
	name: String,
	provider: String,
	effort: Option<String>,
}

async fn title_model(source: &Source, model: NativeThreadModelSettings) -> Option<TitleModel> {
	let provider = model.model_provider?;
	if provider == "openai"
		&& let AgentCapabilitiesResult::Available { models, .. } =
			crate::agent_capabilities::read(&source.client).await
		&& models.iter().any(|m| {
			m.model.as_str() == "gpt-5.6-luna" && m.efforts.iter().any(|e| e.as_str() == "low")
		}) {
		return Some(TitleModel {
			name: "gpt-5.6-luna".into(),
			provider,
			effort: Some("low".into()),
		});
	}
	Some(TitleModel { name: model.model?, provider, effort: None })
}
