//! Identify a completed send in visual capture evidence.

use serde_json::Value;

pub(crate) fn composer_send_answered(before: &Value, evidence: &Value, message: &str) -> bool {
	if evidence["uncertain"] != false || evidence["sending"] != false || evidence["draft"] != "" {
		return false;
	}

	let (Some(owner), Some(previous), Some(entries)) = (
		before["history"][0].as_str(),
		before["history"][1]["entries"].as_array(),
		evidence["history"][1]["entries"].as_array(),
	) else {
		return false;
	};

	if evidence["history"][0] != owner {
		return false;
	}

	entries
		.iter()
		.filter(|entry| {
			entry["kind"] == "user"
				&& entry["text"] == message
				&& entry["id"].as_i64().is_some()
				&& !previous.iter().any(|old| old["id"] == entry["id"])
		})
		.any(|prompt| {
			let turn = prompt["turn_id"]
				.as_str()
				.or_else(|| prompt["receipt"]["delivered_turn_id"].as_str());

			turn.filter(|turn| !turn.is_empty()).is_some_and(|turn| {
				entries.iter().any(|entry| {
					entry["kind"] == "assistant"
						&& entry["id"].as_i64().is_some()
						&& !previous.iter().any(|old| old["id"] == entry["id"])
						&& entry["turn_id"] == turn
						&& entry["text"].as_str().is_some_and(|text| text.contains("UI_READY"))
				})
			})
		})
}

#[cfg(test)]
mod tests {
	use serde_json::{self, Value};

	use crate::capture_send;

	fn evidence() -> Value {
		serde_json::json!({"draft":"", "sending":false, "uncertain":false, "feedback":"",
		"history":["manager", {"outcome":"available", "entries":[
			{"id":1,"kind":"user","text":"Reply UI_READY", "turn_id":"turn"},
			{"id":2,"kind":"assistant","text":"UI_READY", "turn_id":"turn"}
		]}]})
	}

	#[test]
	fn capture_send_accepts_current_history_without_obsolete_feedback() {
		let before =
			serde_json::json!({"history":["manager", {"outcome":"available", "entries":[]}]});

		assert!(capture_send::composer_send_answered(&before, &evidence(), "Reply UI_READY"));

		let mut after = evidence();

		after["history"][1]["entries"][0]["turn_id"] = Value::Null;
		after["history"][1]["entries"][0]["receipt"] =
			serde_json::json!({"delivered_turn_id":"turn"});

		assert!(capture_send::composer_send_answered(&before, &after, "Reply UI_READY"));
	}

	#[test]
	fn capture_send_rejects_old_or_unrelated_answers() {
		let mut after = evidence();

		after["feedback"] = serde_json::json!("Accepted by service");

		assert!(!capture_send::composer_send_answered(&after, &after, "Reply UI_READY"));

		let mut before = evidence();

		before["history"][1]["entries"].as_array_mut().unwrap().remove(0);

		assert!(!capture_send::composer_send_answered(&before, &after, "Reply UI_READY"));

		let before =
			serde_json::json!({"history":["manager", {"outcome":"available", "entries":[]}]});

		after["history"][1]["entries"][1]["turn_id"] = serde_json::json!("other-turn");

		assert!(!capture_send::composer_send_answered(&before, &after, "Reply UI_READY"));
	}

	#[test]
	fn capture_send_requires_settled_delivery_and_same_owner() {
		let before =
			serde_json::json!({"history":["manager", {"outcome":"available", "entries":[]}]});

		for (field, value) in [
			("uncertain", serde_json::json!(true)),
			("sending", serde_json::json!(true)),
			("draft", serde_json::json!("retained")),
		] {
			let mut after = evidence();

			after[field] = value;

			assert!(!capture_send::composer_send_answered(&before, &after, "Reply UI_READY"));
		}

		assert!(!capture_send::composer_send_answered(&Value::Null, &evidence(), "Reply UI_READY"));

		let mut after = evidence();

		after["history"][1]["entries"][0]["turn_id"] = Value::Null;

		assert!(!capture_send::composer_send_answered(&before, &after, "Reply UI_READY"));

		after = evidence();
		after["history"][0] = serde_json::json!("other-manager");

		assert!(!capture_send::composer_send_answered(&before, &after, "Reply UI_READY"));
	}
}
