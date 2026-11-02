//! Service-owned recap requests.
mod excerpts;
use crate::agent_message_history as history;
mod prompt;
mod response;
mod voice;

use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};

use serde_json::Value;
use tokio::sync::watch;

use crate::agent_usage_estimate::{Source, same_owner};
use decodex_codex::app_server_client::{HistoryGuard, ServerEvent};
use decodex_database::{AgentVoiceHistory, AgentVoiceHistoryRevision};
use decodex_protocol::{
	AgentActionDto, EntityId, TaskRecap, TaskRecapPhase, TaskRecapStatus, WireText,
};

#[derive(Clone, Default)]
pub(crate) struct Recaps(Arc<Mutex<State>>);
impl Recaps {
	pub(crate) fn note_input(&self, action: &AgentActionDto) {
		match action {
			AgentActionDto::Send { root_id, .. }
			| AgentActionDto::SendConfigured { root_id, .. } => self.cancel_work(root_id.as_str()),
			AgentActionDto::SendPromptInput { work_id, .. }
			| AgentActionDto::RetryInput { work_id, .. }
			| AgentActionDto::Steer { work_id, .. }
			| AgentActionDto::NativeAgentInput { work_id, .. }
			| AgentActionDto::AnswerQuestion { work_id, .. }
			| AgentActionDto::SkipQuestion { work_id, .. } => self.cancel_work(work_id.as_str()),
			_ => {},
		}
	}

	pub(crate) fn cancel_work(&self, work: &str) {
		if let Ok(mut state) = self.0.lock()
			&& let Some(record) = state.requests.get_mut(work)
		{
			cancel(record);
		}
	}

	pub(crate) fn stop(&self) {
		if let Ok(mut state) = self.0.lock() {
			for record in state.requests.values_mut() {
				cancel(record);
			}
		}
	}

	pub(crate) fn start(
		&self,
		source: Source,
		key: &str,
		voice_revision: AgentVoiceHistoryRevision,
	) -> Result<watch::Receiver<bool>, &'static str> {
		let mut state = self.0.lock().map_err(|_| "Recap service unavailable")?;

		if state
			.requests
			.values()
			.any(|r| matches!(r.phase, TaskRecapPhase::Pending | TaskRecapPhase::Cancelling))
		{
			return Err("A recap is already running or being cancelled");
		}
		if state.requests.len() >= 32 && !state.requests.contains_key(&source.key.work) {
			let oldest = state
				.requests
				.iter()
				.find(|(_, r)| {
					!matches!(r.phase, TaskRecapPhase::Pending | TaskRecapPhase::Cancelling)
				})
				.map(|(key, _)| key.clone())
				.ok_or("Recap capacity is full")?;

			state.requests.remove(&oldest);
		}

		let (send, receive) = watch::channel(false);
		let guard = source.client.thread_settings_guard(&source.key.thread);

		state.requests.insert(
			source.key.work.clone(),
			Record {
				voice_revision,
				source,
				guard,
				key: key.into(),
				cancel: send,
				phase: TaskRecapPhase::Pending,
				recap: None,
			},
		);

		Ok(receive)
	}

	pub(crate) fn cancel_request(&self, work: &str, key: &str) {
		if let Ok(mut state) = self.0.lock()
			&& let Some(record) = state.requests.get_mut(work)
			&& record.key == key
		{
			cancel(record);
		}
	}

	pub(crate) fn voice_is_current(
		&self,
		work: &str,
		revision: &AgentVoiceHistoryRevision,
	) -> bool {
		self.0.lock().is_ok_and(|state| {
			state.requests.get(work).is_none_or(|r| &r.voice_revision == revision)
		})
	}

	pub(crate) fn status(&self, work: EntityId, source: Option<&Source>) -> TaskRecapStatus {
		let mut result = TaskRecapStatus {
			work_id: work.clone(),
			thread_id: None,
			request_id: None,
			phase: TaskRecapPhase::Idle,
			recap: None,
		};

		if let Ok(state) = self.0.lock()
			&& let Some(record) = state.requests.get(work.as_str())
		{
			let current = source.is_some_and(|source| same_owner(&record.source, source))
				&& record.guard.as_ref().is_none_or(|guard| guard.is_live());

			result.thread_id = WireText::new(record.source.key.thread.clone()).ok();
			result.request_id = WireText::new(record.key.clone()).ok();
			result.phase = if current { record.phase } else { TaskRecapPhase::Cancelled };
			result.recap = if current { record.recap.clone() } else { None };
		}

		result
	}

	pub(crate) fn observe(&self, event: &ServerEvent) {
		let Ok(mut state) = self.0.lock() else { return };
		let new_input = matches!(&event,ServerEvent::Notification{method,params} if matches!(method.as_str(),"item/started"|"item/completed") && params["item"]["type"]=="userMessage");
		let (method, thread) = match &event {
			ServerEvent::Notification { method, params } =>
				(method.as_str(), params["threadId"].as_str()),
			ServerEvent::Request { params, .. } => ("request", params["threadId"].as_str()),
			ServerEvent::Closed(_) => {
				for record in state.requests.values_mut() {
					cancel(record);
				}

				return;
			},
			_ => return,
		};

		let Some(thread) = thread else { return };
		if new_input
			|| event.has_voice_transcript()
			|| matches!(
				method,
				"turn/started"
					| "turn/completed"
					| "thread/reverted"
					| "thread/closed"
					| "thread/archived"
					| "thread/deleted"
					| "thread/settings/updated"
			) {
			for record in state.requests.values_mut().filter(|r| r.source.key.thread == thread) {
				cancel(record);
			}
		}
	}

	pub(crate) fn finish(&self, work: &str, key: &str, result: Option<TaskRecap>) {
		let Ok(mut state) = self.0.lock() else { return };
		let Some(record) = state.requests.get_mut(work).filter(|record| record.key == key) else {
			return;
		};
		if *record.cancel.borrow() {
			record.phase = TaskRecapPhase::Cancelled;
			record.recap = None;
		} else {
			record.phase =
				if result.is_some() { TaskRecapPhase::Ready } else { TaskRecapPhase::Failed };
			record.recap = result;
		}
	}
}

pub(crate) struct Prepared {
	pub options: decodex_codex::app_server_client::TemporaryStructuredOptions,
	pub guard: HistoryGuard,
	pub prompt: String,
	pub latest_turn: Option<String>,
}

#[derive(Default)]
struct State {
	requests: BTreeMap<String, Record>,
}

struct Record {
	voice_revision: AgentVoiceHistoryRevision,
	source: Source,
	guard: Option<HistoryGuard>,
	key: String,
	cancel: watch::Sender<bool>,
	phase: TaskRecapPhase,
	recap: Option<TaskRecap>,
}

pub(crate) fn parse(value: &str) -> Option<TaskRecap> {
	response::parse(value)
}

pub(crate) fn schema() -> Value {
	response::schema()
}

pub(crate) async fn prepare(
	source: &Source,
	voice: Option<&AgentVoiceHistory>,
) -> Option<Prepared> {
	let (permissions, guard) = source.client.configured_task_permissions(&source.key.thread)?;
	let model =
		source.client.thread_model_settings(&source.key.thread, guard.clone()).await.ok()??;
	let has_voice = voice.is_some_and(|v| v.revision.calls > 0);
	let history = history::read(&source.client, &source.key.thread, has_voice).await.ok()?;

	if !guard.is_live() {
		return None;
	}

	Some(Prepared {
		guard,
		options: decodex_codex::app_server_client::TemporaryStructuredOptions {
			model: model.model?,
			model_provider: model.model_provider?,
			cwd: permissions.cwd,
			active_permission_profile: permissions.profile_id,
			mcp_server_names: Vec::new(),
		},
		prompt: match voice {
			Some(voice) => self::voice::compose(&history, voice)?,
			None => prompt::build(&excerpts::render(&history.exchanges)),
		},
		latest_turn: history.latest_turn,
	})
}

fn cancel(record: &mut Record) {
	let _ = record.cancel.send(true);

	record.recap = None;
	record.phase = if matches!(record.phase, TaskRecapPhase::Pending | TaskRecapPhase::Cancelling) {
		TaskRecapPhase::Cancelling
	} else {
		TaskRecapPhase::Cancelled
	};
}

#[cfg(test)] mod tests;
