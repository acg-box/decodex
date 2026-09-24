//! Transport-observed request liveness, independent of an owner's event queue.
use super::{ClientError, RequestId, ServerEvent};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
	collections::HashMap,
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
	},
};

#[derive(Clone, Default)]
pub(super) struct ServerRequests(
	Arc<Mutex<HashMap<RequestId, Entry>>>,
	Arc<AtomicU64>,
	Arc<AtomicU64>,
	super::settings_guard::SettingsRevisions,
	super::permission_observations::SettingsObservations<super::NativeTaskPermissions>,
	super::permission_observations::SettingsObservations<super::NativeTaskPlugins>,
	super::permission_observations::SettingsObservations<super::NativeTaskModelSettings>,
);
struct Entry {
	thread: String,
	turn: Option<String>,
	digest: [u8; 32],
}

/// Exact history or question-state version on one native connection.
#[derive(Clone)]
pub struct HistoryGuard {
	requests: ServerRequests,
	revision: u64,
	questions: bool,
	settings: Option<super::settings_guard::SettingsGuard>,
}
impl HistoryGuard {
	/// Connection-local settings revision for review identities. Check `is_live` before use.
	pub fn settings_revision(&self) -> Option<u64> {
		self.settings.as_ref().map(super::settings_guard::SettingsGuard::revision)
	}

	pub(super) fn belongs_to(&self, requests: &ServerRequests) -> bool {
		Arc::ptr_eq(&self.requests.0, &requests.0)
	}

	/// Whether the selected native state is unchanged since this version.
	pub fn is_live(&self) -> bool {
		(if self.questions {
			self.requests.question_revision()
		} else {
			self.requests.history_revision()
		}) == self.revision
			&& self.settings.as_ref().is_none_or(super::settings_guard::SettingsGuard::is_live)
	}
}

/// Exact live request observed on this transport, not transferable between connections.
#[derive(Clone)]
pub struct ServerRequestGuard {
	requests: ServerRequests,
	id: RequestId,
	digest: [u8; 32],
}
impl ServerRequestGuard {
	pub(super) fn matches_id(&self, id: &RequestId) -> bool {
		self.id == *id
	}

	pub(super) fn belongs_to(&self, requests: &ServerRequests) -> bool {
		Arc::ptr_eq(&self.requests.0, &requests.0)
	}

	/// Whether the transport still observes this exact request as unresolved.
	pub fn is_live(&self) -> bool {
		self.requests
			.0
			.lock()
			.is_ok_and(|rows| rows.get(&self.id).is_some_and(|e| e.digest == self.digest))
	}
}

fn digest(method: &str, params: &Value) -> [u8; 32] {
	Sha256::digest(json!([method, params]).to_string().as_bytes()).into()
}
impl ServerRequests {
	pub(super) fn history_guard(&self, revision: u64) -> Option<HistoryGuard> {
		(self.history_revision() == revision).then(|| HistoryGuard {
			requests: self.clone(),
			revision,
			questions: false,
			settings: None,
		})
	}

	pub(super) fn configured_permissions(
		&self,
		thread: &str,
	) -> Option<(super::NativeTaskPermissions, HistoryGuard)> {
		let guard = self.thread_settings_guard(thread)?;
		let settings = self.4.configured(thread)?;
		guard.is_live().then_some((settings, guard))
	}

	pub(super) fn permission_observation(
		&self,
		thread: &str,
	) -> Option<(super::NativeTaskPermissions, HistoryGuard)> {
		let (settings, observed) = self.4.get(thread)?;
		let mut guard = self.history_guard(self.history_revision())?;
		guard.settings = Some(observed);
		Some((settings, guard))
	}

	pub(super) fn configured_plugins(
		&self,
		thread: &str,
	) -> Option<(super::NativeTaskPlugins, HistoryGuard)> {
		let guard = self.thread_settings_guard(thread)?;
		let settings = self.5.configured(thread)?;
		guard.is_live().then_some((settings, guard))
	}

	pub(super) fn plugin_observation(
		&self,
		thread: &str,
	) -> Option<(super::NativeTaskPlugins, HistoryGuard)> {
		let (settings, observed) = self.5.get(thread)?;
		let mut guard = self.history_guard(self.history_revision())?;
		guard.settings = Some(observed);
		Some((settings, guard))
	}

	pub(super) fn configured_models(
		&self,
		thread: &str,
	) -> Option<(super::NativeTaskModelSettings, HistoryGuard)> {
		let guard = self.thread_settings_guard(thread)?;
		let settings = self.6.configured(thread)?;
		guard.is_live().then_some((settings, guard))
	}

	pub(super) fn model_observation(
		&self,
		thread: &str,
	) -> Option<(super::NativeTaskModelSettings, HistoryGuard)> {
		let (settings, observed) = self.6.get(thread)?;
		let mut guard = self.history_guard(self.history_revision())?;
		guard.settings = Some(observed);
		Some((settings, guard))
	}

	pub(super) fn permission_revision(&self) -> u64 {
		self.4.revision()
	}

	pub(super) fn observe_permission_hydration(&self, thread: &str, response: &Value) {
		self.3.invalidate(thread);
		let guard = self.3.capture(thread);
		self.4.record(
			thread,
			super::NativeTaskPermissions::from_thread_response(response),
			guard.clone(),
		);
		self.5.record(thread, super::NativeTaskPlugins::from_settings(response), guard.clone());
		self.6.record(
			thread,
			super::NativeTaskModelSettings::from_thread_response(response),
			guard,
		);
	}

	pub(super) fn question_guard(&self, revision: u64) -> Option<HistoryGuard> {
		(self.question_revision() == revision).then(|| HistoryGuard {
			requests: self.clone(),
			revision,
			questions: true,
			settings: None,
		})
	}

	pub(super) fn thread_settings_guard(&self, thread: &str) -> Option<HistoryGuard> {
		let mut guard = self.history_guard(self.history_revision())?;
		guard.settings = Some(self.3.capture(thread)?);
		Some(guard)
	}

	pub(super) fn with_thread_settings_guard(
		&self,
		thread: &str,
		mut guard: HistoryGuard,
	) -> Option<HistoryGuard> {
		if !guard.belongs_to(self) || !guard.is_live() {
			return None;
		}
		guard.settings = Some(self.3.capture(thread)?);
		Some(guard)
	}

	pub(super) fn question_revision(&self) -> u64 {
		self.2.load(Ordering::Acquire)
	}

	pub(super) fn history_revision(&self) -> u64 {
		self.1.load(Ordering::Acquire)
	}

	pub(super) fn guard(
		&self,
		id: &RequestId,
		method: &str,
		params: &Value,
	) -> Option<ServerRequestGuard> {
		let guard = ServerRequestGuard {
			requests: self.clone(),
			id: id.clone(),
			digest: digest(method, params),
		};
		guard.is_live().then_some(guard)
	}

	pub(super) fn remove(&self, id: &RequestId) {
		if let Ok(mut rows) = self.0.lock() {
			rows.remove(id);
		}
	}

	pub(super) fn clear(&self) {
		self.3.clear();
		self.4.clear();
		self.5.clear();
		self.6.clear();
		if let Ok(mut rows) = self.0.lock() {
			rows.clear();
		}
	}

	fn observe_permission_event(&self, event: &ServerEvent) {
		let ServerEvent::Notification { method, params } = event else { return };
		if method == "thread/reverted" {
			self.4.clear();
			self.5.clear();
			self.6.clear();
		}
		let Some(thread) = params["threadId"].as_str() else { return };
		if matches!(
			method.as_str(),
			"thread/settings/updated"
				| "thread/closed"
				| "thread/archived"
				| "thread/deleted"
				| "turn/started"
		) {
			self.3.invalidate(thread);
		}
		match method.as_str() {
			"thread/settings/updated" => self.5.record(
				thread,
				super::NativeTaskPlugins::from_settings(&params["threadSettings"]),
				self.3.capture(thread),
			),
			"turn/started" => self.5.start_turn(thread, params["turn"]["id"].as_str()),
			"turn/completed" =>
				self.5.finish_turn(thread, params["turn"]["id"].as_str(), self.3.capture(thread)),
			"thread/closed" | "thread/archived" | "thread/deleted" => self.5.remove(thread),
			_ => {},
		}

		match method.as_str() {
			"thread/settings/updated" => self.6.record(
				thread,
				super::NativeTaskModelSettings::from_notification(&params["threadSettings"]),
				self.3.capture(thread),
			),
			"turn/started" => self.6.start_turn(thread, params["turn"]["id"].as_str()),
			"turn/completed" =>
				self.6.finish_turn(thread, params["turn"]["id"].as_str(), self.3.capture(thread)),
			"thread/closed" | "thread/archived" | "thread/deleted" => self.6.remove(thread),
			_ => {},
		}

		match method.as_str() {
			"thread/settings/updated" => self.4.record(
				thread,
				super::NativeTaskPermissions::from_notification(&params["threadSettings"]),
				self.3.capture(thread),
			),
			"turn/started" => self.4.start_turn(thread, params["turn"]["id"].as_str()),
			"turn/completed" =>
				self.4.finish_turn(thread, params["turn"]["id"].as_str(), self.3.capture(thread)),
			"thread/closed" | "thread/archived" | "thread/deleted" => self.4.remove(thread),
			_ => {},
		}
	}

	pub(super) fn observe(&self, event: &ServerEvent) -> Result<(), ClientError> {
		self.observe_permission_event(event);
		let mut rows = self.0.lock().map_err(|_| ClientError::Closed)?;
		if let ServerEvent::Notification { method, params } = event
			&& invalidates_question_state(method, params)
		{
			self.2.fetch_add(1, Ordering::AcqRel);
		}
		if let ServerEvent::Notification { method, params } = event
			&& method == "thread/reverted"
			&& params["threadId"].as_str().is_some_and(|id| !id.is_empty())
		{
			self.1.fetch_add(1, Ordering::AcqRel);
		}

		match event {
			ServerEvent::Request { id, method, params } => {
				if let Some(thread) = params["threadId"].as_str() {
					if rows.len() >= 256 {
						return Err(ClientError::CapacityExceeded);
					}
					if rows.contains_key(id) {
						return Err(ClientError::InvalidFrame);
					}
					rows.insert(
						id.clone(),
						Entry {
							thread: thread.into(),
							turn: params["turnId"].as_str().map(str::to_owned),
							digest: digest(method, params),
						},
					);
				}
			},
			ServerEvent::Notification { method, params } if method == "serverRequest/resolved" =>
				if let (Some(thread), Ok(id)) = (
					params["threadId"].as_str(),
					serde_json::from_value::<RequestId>(params["requestId"].clone()),
				) && rows.get(&id).is_some_and(|e| e.thread == thread)
				{
					rows.remove(&id);
				},
			ServerEvent::Notification { method, params }
				if ["thread/closed", "thread/archived", "thread/deleted", "thread/reverted"]
					.contains(&method.as_str()) =>
				if let Some(thread) = params["threadId"].as_str() {
					rows.retain(|_, e| e.thread != thread);
				},
			ServerEvent::Notification { method, params } if method == "turn/completed" => {
				if let (Some(thread), Some(turn)) =
					(params["threadId"].as_str(), params["turn"]["id"].as_str())
				{
					rows.retain(|_, e| e.thread != thread || e.turn.as_deref() != Some(turn));
				}
			},
			_ => {},
		}
		Ok(())
	}
}

/// Whether a committed native input or revert can invalidate a pending question.
pub fn invalidates_question_state(method: &str, params: &Value) -> bool {
	params["threadId"].as_str().is_some_and(|id| !id.is_empty())
		&& (method == "thread/reverted"
			|| (method == "item/completed"
				&& params["item"]["type"] == "userMessage"
				&& params["turnId"].as_str().is_some_and(|id| !id.is_empty())
				&& params["item"]["id"].as_str().is_some_and(|id| !id.is_empty())))
}
