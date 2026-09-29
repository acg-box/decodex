//! Observe native thread plugin exclusions without exposing a local selection write.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

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
	fn plugin_observations_distinguish_missing_invalid_and_empty_exclusions() {
		assert!(NativeTaskPlugins::from_settings(&json!({})).is_none());
		assert!(
			NativeTaskPlugins::from_settings(&json!({"disabledPluginIds":[]}))
				.unwrap()
				.disabled_plugin_ids
				.is_empty()
		);
		assert_eq!(
			NativeTaskPlugins::from_settings(
				&json!({"disabledPluginIds":["sample@market","other@market"]})
			)
			.unwrap()
			.disabled_plugin_ids,
			["sample@market", "other@market"]
		);
		for list in [
			json!(null),
			json!(["same", "same"]),
			json!(["bad\n"]),
			json!([42]),
			json!(vec!["x"; 129]),
		] {
			assert!(NativeTaskPlugins::from_settings(&json!({"disabledPluginIds":list})).is_none());
		}
	}
}
