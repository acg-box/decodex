//! Thread-local plugin selection. Native settings own persistence and turn activation.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};

fn valid_id(value: &str) -> bool {
	!value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

fn valid_list(ids: &[String]) -> bool {
	ids.len() <= 128
		&& ids.iter().map(String::len).sum::<usize>() <= 32 * 1024
		&& ids.iter().all(|id| valid_id(id))
		&& ids.iter().collect::<HashSet<_>>().len() == ids.len()
}

/// Saved selection for subsequent turns, not proof of the active tool catalog.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NativeTaskPlugins {
	/// Canonical plugin IDs; an explicitly reported empty list means none excluded.
	pub disabled_plugin_ids: Vec<String>,
}

impl NativeTaskPlugins {
	/// Project a settings notification or top-level start/resume response.
	/// Missing data is unknown, never an implicit empty selection.
	pub fn from_settings(value: &Value) -> Option<Self> {
		let ids: Vec<String> =
			serde_json::from_value(value.get("disabledPluginIds")?.clone()).ok()?;
		valid_list(&ids).then_some(Self { disabled_plugin_ids: ids })
	}
}

/// Exact replacement; the runtime must preserve unrelated exclusions from a fresh observation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ThreadPluginSelection {
	thread_id: String,
	disabled_plugin_ids: Vec<String>,
}

impl ThreadPluginSelection {
	/// Construct one bounded native replacement. An empty list explicitly clears exclusions.
	pub fn new(thread: &str, disabled_plugin_ids: Vec<String>) -> Result<Self, ClientError> {
		let selection = Self { thread_id: thread.into(), disabled_plugin_ids };
		if selection.valid() { Ok(selection) } else { Err(ClientError::InvalidFrame) }
	}

	fn valid(&self) -> bool {
		valid_id(&self.thread_id) && valid_list(&self.disabled_plugin_ids)
	}
}

/// Allow only the narrow thread-local update through the retained native bridge.
pub fn is_thread_plugin_selection(value: &Value) -> bool {
	serde_json::from_value::<ThreadPluginSelection>(value.clone()).is_ok_and(|v| v.valid())
}

/// Native queued the change. A settings publication must confirm the saved selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadPluginSelectionQueued;

impl AppServerClient {
	/// Dispatch once with the observed settings guard. Never retry an unknown reply.
	pub async fn queue_thread_plugin_selection(
		&self,
		selection: &ThreadPluginSelection,
		guard: HistoryGuard,
	) -> Result<ThreadPluginSelectionQueued, ClientError> {
		if !selection.valid() {
			return Err(ClientError::InvalidFrame);
		}
		let params = serde_json::to_value(selection).map_err(|_| ClientError::InvalidFrame)?;
		let response = tokio::time::timeout(
			Duration::from_secs(8),
			self.request_with_history("thread/settings/update", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if response.as_object().is_some_and(|v| v.is_empty()) {
			Ok(ThreadPluginSelectionQueued)
		} else {
			Err(ClientError::InvalidFrame)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn plugin_publications_invalidate_old_guards_and_do_not_revive_missing_facts() {
		use super::super::{ServerEvent, ServerRequests};
		let requests = ServerRequests::default();
		let publish = |value| {
			requests
				.observe(&ServerEvent::Notification {
					method: "thread/settings/updated".into(),
					params: json!({"threadId":"task","threadSettings":value}),
				})
				.unwrap()
		};
		publish(json!({"disabledPluginIds":["one@market"]}));
		let (_, old) = requests.plugin_observation("task").unwrap();
		publish(json!({"disabledPluginIds":["two@market"]}));
		assert!(!old.is_live());
		let (next, _) = requests.plugin_observation("task").unwrap();
		assert_eq!(next.disabled_plugin_ids, ["two@market"]);
		requests
			.observe(&ServerEvent::Notification {
				method: "turn/started".into(),
				params: json!({"threadId":"task","turn":{"id":"turn"}}),
			})
			.unwrap();
		assert!(requests.plugin_observation("task").is_none());
		publish(json!({"disabledPluginIds":[]}));
		assert!(requests.plugin_observation("task").is_none());
		requests
			.observe(&ServerEvent::Notification {
				method: "turn/completed".into(),
				params: json!({"threadId":"task","turn":{"id":"turn"}}),
			})
			.unwrap();
		assert!(requests.plugin_observation("task").unwrap().0.disabled_plugin_ids.is_empty());
		publish(json!({}));
		assert!(requests.plugin_observation("task").is_none());
		requests
			.observe_permission_hydration("task", &json!({"disabledPluginIds":["cold@market"]}));
		assert_eq!(
			requests.plugin_observation("task").unwrap().0.disabled_plugin_ids,
			["cold@market"]
		);
		requests.clear();
		assert!(requests.plugin_observation("task").is_none());
	}

	#[test]
	fn plugin_selection_preserves_unknown_and_rejects_unrelated_writes() {
		assert!(NativeTaskPlugins::from_settings(&json!({})).is_none());
		assert!(NativeTaskPlugins::from_settings(&json!({"disabledPluginIds":null})).is_none());
		assert!(
			NativeTaskPlugins::from_settings(&json!({"disabledPluginIds":[]}))
				.unwrap()
				.disabled_plugin_ids
				.is_empty()
		);
		let value = serde_json::to_value(
			ThreadPluginSelection::new("task", vec!["sample@market".into(), "other@market".into()])
				.unwrap(),
		)
		.unwrap();
		assert!(is_thread_plugin_selection(&value));
		for field in ["model", "permissions", "approvalPolicy", "serviceTier"] {
			let mut widened = value.clone();
			widened[field] = json!("override");
			assert!(!is_thread_plugin_selection(&widened));
		}
		for list in [
			json!(null),
			json!(["same", "same"]),
			json!(["bad\n"]),
			json!([42]),
			json!(vec!["x"; 129]),
		] {
			let mut malformed = value.clone();
			malformed["disabledPluginIds"] = list;
			assert!(!is_thread_plugin_selection(&malformed));
			assert!(NativeTaskPlugins::from_settings(&malformed).is_none());
		}
		let empty =
			serde_json::to_value(ThreadPluginSelection::new("task", vec![]).unwrap()).unwrap();
		assert_eq!(empty, json!({"threadId":"task","disabledPluginIds":[]}));
	}

	#[tokio::test]
	async fn plugin_update_uses_one_guarded_request_and_only_reports_queued() {
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		let (local, remote) = tokio::io::duplex(4096);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let frame: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(frame["method"], "thread/settings/update");
			assert_eq!(
				frame["params"],
				json!({"threadId":"task","disabledPluginIds":["sample@market"]})
			);
			writer
				.write_all(format!("{}\n", json!({"id":frame["id"],"result":{}})).as_bytes())
				.await
				.unwrap();
		});
		let guard = client.thread_settings_guard("task").unwrap();
		assert_eq!(
			client
				.queue_thread_plugin_selection(
					&ThreadPluginSelection::new("task", vec!["sample@market".into()]).unwrap(),
					guard
				)
				.await
				.unwrap(),
			ThreadPluginSelectionQueued
		);
		server.await.unwrap();
	}
}
