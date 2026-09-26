//! Service-owned recap requests and exact native event routing.
mod excerpts;
mod history;
mod prompt;
mod response;

use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::ServerEvent;
use decodex_protocol::{EntityId, TaskRecap, TaskRecapPhase as Phase, TaskRecapStatus, WireText};
use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Default)]
pub(crate) struct Recaps(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
	requests: BTreeMap<String, Record>,
	routes: BTreeMap<String, mpsc::Sender<ServerEvent>>,
}
struct Record {
	source: Source,
	guard: Option<decodex_codex::app_server_client::HistoryGuard>,
	key: String,
	cancel: watch::Sender<bool>,
	phase: Phase,
	recap: Option<TaskRecap>,
}

pub(crate) fn same_owner(a: &Source, b: &Source) -> bool {
	b.client.history_guard(b.client.history_revision()).is_some()
		&& a.key.work == b.key.work
		&& a.key.thread == b.key.thread
		&& a.key.account == b.key.account
		&& a.key.generation == b.key.generation
		&& a.key.revision == b.key.revision
		&& a.client.connection_identity() == b.client.connection_identity()
}
fn cancel(record: &mut Record) {
	let _ = record.cancel.send(true);
	record.recap = None;
	record.phase = if matches!(record.phase, Phase::Pending | Phase::Cancelling) {
		Phase::Cancelling
	} else {
		Phase::Cancelled
	};
}

impl Recaps {
	pub(crate) fn note_input(&self, action: &decodex_protocol::ChiefActionDto) {
		use decodex_protocol::ChiefActionDto as Action;
		match action {
			Action::Send { root_id, .. } | Action::SendConfigured { root_id, .. } =>
				self.cancel_work(root_id.as_str()),
			Action::Steer { work_id, .. }
			| Action::NativeAgentInput { work_id, .. }
			| Action::AnswerQuestion { work_id, .. }
			| Action::SkipQuestion { work_id, .. } => self.cancel_work(work_id.as_str()),
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
			state.routes.clear();
		}
	}

	pub(crate) fn start(
		&self,
		source: Source,
		key: &str,
	) -> Result<watch::Receiver<bool>, &'static str> {
		let mut state = self.0.lock().map_err(|_| "Recap service unavailable")?;
		if state.requests.values().any(|r| matches!(r.phase, Phase::Pending | Phase::Cancelling)) {
			return Err("A recap is already running or being cancelled");
		}
		if state.requests.len() >= 32 && !state.requests.contains_key(&source.key.work) {
			let oldest = state
				.requests
				.iter()
				.find(|(_, r)| !matches!(r.phase, Phase::Pending | Phase::Cancelling))
				.map(|(key, _)| key.clone())
				.ok_or("Recap capacity is full")?;
			state.requests.remove(&oldest);
		}
		let (send, receive) = watch::channel(false);
		let guard = source.client.thread_settings_guard(&source.key.thread);
		state.requests.insert(
			source.key.work.clone(),
			Record {
				source,
				guard,
				key: key.into(),
				cancel: send,
				phase: Phase::Pending,
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

	pub(crate) fn status(&self, work: EntityId, source: Option<&Source>) -> TaskRecapStatus {
		let mut result = TaskRecapStatus {
			work_id: work.clone(),
			thread_id: None,
			request_id: None,
			phase: Phase::Idle,
			recap: None,
		};
		if let Ok(state) = self.0.lock()
			&& let Some(record) = state.requests.get(work.as_str())
		{
			let current = source.is_some_and(|source| same_owner(&record.source, source))
				&& record.guard.as_ref().is_none_or(|guard| guard.is_live());
			result.thread_id = WireText::new(record.source.key.thread.clone()).ok();
			result.request_id = WireText::new(record.key.clone()).ok();
			result.phase = if current { record.phase } else { Phase::Cancelled };
			result.recap = if current { record.recap.clone() } else { None };
		}
		result
	}

	pub(crate) fn route(&self, event: ServerEvent) -> Option<ServerEvent> {
		let Ok(mut state) = self.0.lock() else { return Some(event) };
		let new_input = matches!(&event,ServerEvent::Notification{method,params} if matches!(method.as_str(),"item/started"|"item/completed") && params["item"]["type"]=="userMessage");
		let (method, thread) = match &event {
			ServerEvent::Notification { method, params } =>
				(method.as_str(), params["threadId"].as_str()),
			ServerEvent::Request { params, .. } => ("request", params["threadId"].as_str()),
			ServerEvent::Closed(_) => {
				for record in state.requests.values_mut() {
					cancel(record);
				}
				state.routes.clear();
				return Some(event);
			},
			_ => return Some(event),
		};
		if let Some(thread) = thread {
			if let Some(route) = state.routes.get(thread) {
				let thread = thread.to_owned();
				if route.try_send(event).is_err() {
					state.routes.remove(&thread);
				}
				return None;
			}
			if new_input
				|| event.has_voice_transcript()
				|| matches!(
					method,
					"turn/started"
						| "turn/completed" | "thread/reverted"
						| "thread/closed" | "thread/archived"
						| "thread/deleted" | "thread/settings/updated"
				) {
				for record in state.requests.values_mut().filter(|r| r.source.key.thread == thread)
				{
					cancel(record);
				}
			}
		}
		Some(event)
	}

	pub(crate) fn register(&self, thread: &str) -> Option<mpsc::Receiver<ServerEvent>> {
		let mut state = self.0.lock().ok()?;
		if state.routes.len() >= 32 || state.routes.contains_key(thread) {
			return None;
		}
		let (send, receive) = mpsc::channel(64);
		state.routes.insert(thread.into(), send);
		Some(receive)
	}

	pub(crate) fn finish(
		&self,
		work: &str,
		key: &str,
		thread: Option<&str>,
		result: Option<TaskRecap>,
	) {
		if let Ok(mut state) = self.0.lock() {
			if let Some(thread) = thread {
				state.routes.remove(thread);
			}
			if let Some(record) = state.requests.get_mut(work)
				&& record.key == key
			{
				if *record.cancel.borrow() {
					record.phase = Phase::Cancelled;
					record.recap = None;
				} else {
					record.phase = if result.is_some() { Phase::Ready } else { Phase::Failed };
					record.recap = result;
				}
			}
		}
	}
}

pub(crate) struct Prepared {
	pub options: decodex_codex::app_server_client::TemporaryStructuredOptions,
	pub guard: decodex_codex::app_server_client::HistoryGuard,
	pub prompt: String,
	pub latest_turn: Option<String>,
}
pub(crate) async fn prepare(source: &Source) -> Option<Prepared> {
	let (permissions, guard) = source.client.configured_task_permissions(&source.key.thread)?;
	let model =
		source.client.thread_model_settings(&source.key.thread, guard.clone()).await.ok()??;
	let history = history::read(&source.client, &source.key.thread).await.ok()?;
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
		prompt: history.prompt,
		latest_turn: history.latest_turn,
	})
}

pub(crate) fn parse(value: &str) -> Option<TaskRecap> {
	response::parse(value)
}
pub(crate) fn schema() -> serde_json::Value {
	response::schema()
}

#[cfg(test)] mod tests;
