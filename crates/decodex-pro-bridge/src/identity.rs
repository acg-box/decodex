//! Identity comes from an observed ChatGPT response, never from a requested UI label.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionIdentity {
	/// Actual assistant message ID returned by ChatGPT.
	pub message_id: String,
	/// Actual model slug returned for that assistant message.
	pub model: String,
	/// Explicit response mode, when provided by ChatGPT.
	pub mode: Option<String>,
}

impl ExecutionIdentity {
	/// Requested models, subscription plans and the answer's text are not identity evidence.
	pub fn is_pro(&self) -> bool {
		self.model.split(['-', '_']).any(|part| part.eq_ignore_ascii_case("pro"))
			|| self.mode.as_deref().is_some_and(|mode| mode.eq_ignore_ascii_case("pro"))
	}
}

pub(crate) fn from_response(body: &str) -> Vec<ExecutionIdentity> {
	let mut identities = Vec::new();
	if let Ok(value) = serde_json::from_str::<Value>(body) {
		collect(&value, &mut identities);
	} else {
		for line in body.lines().filter_map(|line| line.strip_prefix("data:")) {
			if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
				collect(&value, &mut identities);
			}
		}
	}
	identities
}

fn collect(value: &Value, identities: &mut Vec<ExecutionIdentity>) {
	if value["author"]["role"] == "assistant"
		&& let (Some(id), Some(model)) =
			(value["id"].as_str(), value["metadata"]["model_slug"].as_str())
		&& !id.is_empty()
		&& !model.is_empty()
	{
		let metadata = &value["metadata"];
		let mode = metadata["reasoning_effort"]
			.as_str()
			.or(metadata["thinking_mode"].as_str())
			.map(str::to_owned);
		identities.push(ExecutionIdentity { message_id: id.into(), model: model.into(), mode });
	}
	if let Some(message) = value.get("message") {
		collect(message, identities);
	}
	if let Some(mapping) = value.get("mapping").and_then(Value::as_object) {
		for node in mapping.values() {
			collect(node, identities);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn actual_response_model_is_distinct_from_requested_model_and_prose() {
		let body = r#"{"message":{"id":"answer","author":{"role":"assistant"},"metadata":{"model_slug":"gpt-5","requested_model_slug":"gpt-5-pro"},"content":{"parts":["I am Pro"]}}}"#;
		let identities = from_response(body);
		assert_eq!(identities.len(), 1);
		assert!(!identities[0].is_pro());
		let body = body.replace("\"model_slug\":\"gpt-5\"", "\"model_slug\":\"gpt-5-pro\"");
		assert!(from_response(&body)[0].is_pro());
	}

	#[test]
	fn restored_conversation_and_sse_bind_identity_to_the_assistant_message() {
		let message = r#"{"message":{"id":"answer-2","author":{"role":"assistant"},"metadata":{"model_slug":"gpt-6","thinking_mode":"pro"}}}"#;
		let history = format!("{{\"mapping\":{{\"node\":{message}}}}}");
		let sse = format!("data: {message}\n\ndata: [DONE]\n");
		assert_eq!(from_response(&history), from_response(&sse));
		assert!(from_response(&sse)[0].is_pro());
		assert!(
			from_response(
				r#"{"author":{"role":"user"},"id":"u","metadata":{"model_slug":"gpt-5-pro"}}"#
			)
			.is_empty()
		);
	}
}
