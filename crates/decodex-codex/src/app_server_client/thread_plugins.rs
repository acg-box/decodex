//! Observe native thread plugin exclusions without exposing a local selection write.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Saved selection for subsequent turns, not proof of the active tool catalog.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

fn valid_id(value: &str) -> bool {
	!value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

fn valid_list(ids: &[String]) -> bool {
	ids.len() <= 128
		&& ids.iter().map(String::len).sum::<usize>() <= 32 * 1_024
		&& ids.iter().all(|id| valid_id(id))
		&& ids.iter().collect::<HashSet<_>>().len() == ids.len()
}

#[cfg(test)]
mod tests {
	use serde_json;

	use crate::app_server_client::{
		ServerEvent, ServerRequests, thread_plugins::NativeTaskPlugins,
	};

	#[test]
	fn plugin_publications_invalidate_old_guards_and_do_not_revive_missing_facts() {
		let requests = ServerRequests::default();
		let publish = |value| {
			requests
				.observe(&ServerEvent::Notification {
					method: "thread/settings/updated".into(),
					params: serde_json::json!({"threadId":"task","threadSettings":value}),
				})
				.unwrap()
		};

		publish(serde_json::json!({"disabledPluginIds":["one@market"]}));

		let (_, old) = requests.plugin_observation("task").unwrap();

		publish(serde_json::json!({"disabledPluginIds":["two@market"]}));

		assert!(!old.is_live());

		let (next, _) = requests.plugin_observation("task").unwrap();

		assert_eq!(next.disabled_plugin_ids, ["two@market"]);

		requests
			.observe(&ServerEvent::Notification {
				method: "turn/started".into(),
				params: serde_json::json!({"threadId":"task","turn":{"id":"turn"}}),
			})
			.unwrap();

		assert!(requests.plugin_observation("task").is_none());

		publish(serde_json::json!({"disabledPluginIds":[]}));

		assert!(requests.plugin_observation("task").is_none());

		requests
			.observe(&ServerEvent::Notification {
				method: "turn/completed".into(),
				params: serde_json::json!({"threadId":"task","turn":{"id":"turn"}}),
			})
			.unwrap();

		assert!(requests.plugin_observation("task").unwrap().0.disabled_plugin_ids.is_empty());

		publish(serde_json::json!({}));

		assert!(requests.plugin_observation("task").is_none());

		requests.observe_permission_hydration(
			"task",
			&serde_json::json!({"disabledPluginIds":["cold@market"]}),
		);

		assert_eq!(
			requests.plugin_observation("task").unwrap().0.disabled_plugin_ids,
			["cold@market"]
		);

		requests.clear();

		assert!(requests.plugin_observation("task").is_none());
	}

	#[test]
	fn plugin_observations_distinguish_missing_invalid_and_empty_exclusions() {
		assert!(NativeTaskPlugins::from_settings(&serde_json::json!({})).is_none());
		assert!(
			NativeTaskPlugins::from_settings(&serde_json::json!({"disabledPluginIds":[]}))
				.unwrap()
				.disabled_plugin_ids
				.is_empty()
		);
		assert_eq!(
			NativeTaskPlugins::from_settings(
				&serde_json::json!({"disabledPluginIds":["sample@market","other@market"]})
			)
			.unwrap()
			.disabled_plugin_ids,
			["sample@market", "other@market"]
		);

		for list in [
			serde_json::json!(null),
			serde_json::json!(["same", "same"]),
			serde_json::json!(["bad\n"]),
			serde_json::json!([42]),
			serde_json::json!(vec!["x"; 129]),
		] {
			assert!(
				NativeTaskPlugins::from_settings(&serde_json::json!({"disabledPluginIds":list}))
					.is_none()
			);
		}
	}
}
