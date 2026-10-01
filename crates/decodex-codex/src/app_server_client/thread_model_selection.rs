//! Future-turn model selection. Native settings own persistence and activation.
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time;

use crate::app_server_client::{AppServerClient, ClientError, HistoryGuard};

/// Select a configured model without replacing tier, provider or collaboration settings.
/// The caller must validate the selection against the native model catalog.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ThreadModelSelection {
	thread_id: String,
	model: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	effort: Option<String>,
}
impl ThreadModelSelection {
	/// Omitted effort preserves the native setting; it does not clear it.
	pub fn new(thread: &str, model: &str, effort: Option<String>) -> Result<Self, ClientError> {
		let selection = Self { thread_id: thread.into(), model: model.into(), effort };

		if selection.valid() { Ok(selection) } else { Err(ClientError::InvalidFrame) }
	}

	fn valid(&self) -> bool {
		let valid = |text: &str, limit| {
			!text.trim().is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
		};

		valid(&self.thread_id, 512)
			&& valid(&self.model, 256)
			&& self.model != "gpt-reserve"
			&& self.effort.as_deref().is_none_or(|effort| valid(effort, 128))
	}
}

/// Native queued the change. A settings publication must confirm the configured selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadModelSelectionQueued;

impl AppServerClient {
	/// Dispatch once with the observed settings guard. Never retry an unknown reply.
	pub async fn queue_thread_model_selection(
		&self,
		selection: &ThreadModelSelection,
		guard: HistoryGuard,
	) -> Result<ThreadModelSelectionQueued, ClientError> {
		if !selection.valid() {
			return Err(ClientError::InvalidFrame);
		}

		let params = serde_json::to_value(selection).map_err(|_| ClientError::InvalidFrame)?;
		let response = time::timeout(
			Duration::from_secs(8),
			self.request_with_history("thread/settings/update", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;

		if response.as_object().is_some_and(|v| v.is_empty()) {
			Ok(ThreadModelSelectionQueued)
		} else {
			Err(ClientError::InvalidFrame)
		}
	}
}

/// Allow only a model selection through the retained native bridge.
pub fn is_thread_model_selection(value: &Value) -> bool {
	serde_json::from_value::<ThreadModelSelection>(value.clone()).is_ok_and(|v| v.valid())
}

#[cfg(test)]
mod tests {
	use serde_json;
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use crate::app_server_client::{
		ServerEvent, ServerRequests,
		thread_model_selection::{
			self, AppServerClient, ThreadModelSelection, ThreadModelSelectionQueued, Value,
		},
	};

	#[test]
	fn ordinary_model_selection_rejects_reserve() {
		assert!(ThreadModelSelection::new("task", "gpt-reserve", Some("medium".into())).is_err());
		assert!(!thread_model_selection::is_thread_model_selection(
			&serde_json::json!({"threadId":"task","model":"gpt-reserve"})
		));
	}

	#[test]
	fn model_selection_preserves_other_settings_and_native_effort_spelling() {
		let effort = "future-provider-reasoning-effort-over-32-bytes";
		let selected = serde_json::to_value(
			ThreadModelSelection::new("task", "future-model", Some(effort.into())).unwrap(),
		)
		.unwrap();

		assert_eq!(
			selected,
			serde_json::json!({"threadId":"task","model":"future-model","effort":effort})
		);
		assert!(thread_model_selection::is_thread_model_selection(&selected));

		let preserved =
			serde_json::to_value(ThreadModelSelection::new("task", "future-model", None).unwrap())
				.unwrap();

		assert_eq!(preserved, serde_json::json!({"threadId":"task","model":"future-model"}));

		for field in [
			"serviceTier",
			"modelProvider",
			"collaborationMode",
			"permissions",
			"disabledPluginIds",
		] {
			let mut widened = selected.clone();

			widened[field] = Value::Null;

			assert!(!thread_model_selection::is_thread_model_selection(&widened));
		}
		for field in ["threadId", "model", "effort"] {
			let mut malformed = selected.clone();

			malformed[field] = serde_json::json!("\n");

			assert!(!thread_model_selection::is_thread_model_selection(&malformed));
		}
	}

	#[test]
	fn model_observations_keep_configured_and_active_settings_distinct() {
		let requests = ServerRequests::default();
		let emit = |method: &str, params| {
			requests.observe(&ServerEvent::Notification { method: method.into(), params }).unwrap()
		};
		let settings = |model| serde_json::json!({"model":model,"modelProvider":"fixture","effort":null,"serviceTier":null});

		emit(
			"thread/settings/updated",
			serde_json::json!({"threadId":"task","threadSettings":settings("a")}),
		);

		let (_, old) = requests.model_observation("task").unwrap();
		let revision = old.settings_revision();

		emit("turn/started", serde_json::json!({"threadId":"task","turn":{"id":"turn"}}));

		assert!(!old.is_live());
		assert!(requests.model_observation("task").is_none());

		emit(
			"thread/settings/updated",
			serde_json::json!({"threadId":"task","threadSettings":settings("b")}),
		);

		assert_eq!(requests.configured_models("task").unwrap().0.model, "b");
		assert!(requests.model_observation("task").is_none());

		emit("turn/completed", serde_json::json!({"threadId":"task","turn":{"id":"other"}}));

		assert!(requests.model_observation("task").is_none());

		emit("turn/completed", serde_json::json!({"threadId":"task","turn":{"id":"turn"}}));

		assert_eq!(requests.model_observation("task").unwrap().0.model, "b");

		emit(
			"thread/settings/updated",
			serde_json::json!({"threadId":"task","threadSettings":settings("a")}),
		);

		assert_ne!(requests.model_observation("task").unwrap().1.settings_revision(), revision);

		emit("thread/settings/updated", serde_json::json!({"threadId":"task","threadSettings":{}}));

		assert!(requests.configured_models("task").is_none());

		let mut hydrated = settings("cold");

		hydrated.as_object_mut().unwrap().remove("effort");

		hydrated["reasoningEffort"] = Value::Null;

		requests.observe_permission_hydration("task", &hydrated);

		assert_eq!(requests.model_observation("task").unwrap().0.model, "cold");

		emit("thread/archived", serde_json::json!({"threadId":"task"}));

		assert!(requests.configured_models("task").is_none());
	}

	#[tokio::test]
	async fn model_update_sends_one_guarded_request_and_only_reports_queued() {
		let (local, remote) = io::duplex(4_096);
		let (reader, writer) = io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let frame: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			assert_eq!(frame["method"], "thread/settings/update");
			assert_eq!(
				frame["params"],
				serde_json::json!({"threadId":"task","model":"future-model"})
			);

			writer
				.write_all(
					format!("{}\n", serde_json::json!({"id":frame["id"],"result":{}})).as_bytes(),
				)
				.await
				.unwrap();
		});
		let guard = client.thread_settings_guard("task").unwrap();

		assert_eq!(
			client
				.queue_thread_model_selection(
					&ThreadModelSelection::new("task", "future-model", None).unwrap(),
					guard,
				)
				.await
				.unwrap(),
			ThreadModelSelectionQueued
		);

		server.await.unwrap();
	}
}
