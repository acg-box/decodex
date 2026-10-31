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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeMessage {
	pub id: String,
	pub role: String,
	pub text: String,
	pub complete: bool,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct ObservedData {
	pub identities: Vec<ExecutionIdentity>,
	pub messages: Vec<NativeMessage>,
}

pub(crate) fn from_response(body: &str) -> ObservedData {
	let mut data = ObservedData::default();
	if let Ok(value) = serde_json::from_str::<Value>(body) {
		collect(&value, &mut data);
	} else {
		for line in body.lines().filter_map(|line| line.strip_prefix("data:")) {
			if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
				collect(&value, &mut data);
			}
		}
	}
	data
}

fn collect(value: &Value, data: &mut ObservedData) {
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
		data.identities.push(ExecutionIdentity {
			message_id: id.into(),
			model: model.into(),
			mode,
		});
	}
	if let (Some(id), Some(role), Some(parts)) = (
		value["id"].as_str(),
		value["author"]["role"].as_str(),
		value["content"]["parts"].as_array(),
	) && !id.is_empty()
		&& matches!(role, "user" | "assistant")
		&& let Some(parts) = parts.iter().map(Value::as_str).collect::<Option<Vec<_>>>()
	{
		data.messages.push(NativeMessage {
			id: id.into(),
			role: role.into(),
			text: parts.concat(),
			complete: role == "user"
				|| value["end_turn"] == true
				|| value["status"] == "finished_successfully",
		});
	}
	match value {
		Value::Object(object) =>
			for nested in object.values() {
				collect(nested, data);
			},
		Value::Array(array) =>
			for nested in array {
				collect(nested, data);
			},
		_ => {},
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn actual_response_model_is_distinct_from_requested_model_and_prose() {
		let body = r#"{"message":{"id":"answer","author":{"role":"assistant"},"metadata":{"model_slug":"gpt-5","requested_model_slug":"gpt-5-pro"},"content":{"parts":["I am Pro"]}}}"#;
		let identities = from_response(body).identities;
		assert_eq!(identities.len(), 1);
		assert!(!identities[0].is_pro());
		let body = body.replace("\"model_slug\":\"gpt-5\"", "\"model_slug\":\"gpt-5-pro\"");
		assert!(from_response(&body).identities[0].is_pro());
	}

	#[test]
	fn restored_conversation_and_sse_bind_identity_to_the_assistant_message() {
		let message = r#"{"message":{"id":"answer-2","author":{"role":"assistant"},"metadata":{"model_slug":"gpt-6","thinking_mode":"pro"}}}"#;
		let history = format!("{{\"mapping\":{{\"node\":{message}}}}}");
		let sse = format!("data: {message}\n\ndata: [DONE]\n");
		assert_eq!(from_response(&history), from_response(&sse));
		assert!(from_response(&sse).identities[0].is_pro());
		assert!(
			from_response(
				r#"{"author":{"role":"user"},"id":"u","metadata":{"model_slug":"gpt-5-pro"}}"#
			)
			.identities
			.is_empty()
		);
	}

	#[test]
	fn raw_messages_preserve_markdown_and_do_not_promote_default_models() {
		let body = r#"{"default_model_slug":"gpt-6-pro","data":{"messages":[{"id":"u","author":{"role":"user"},"content":{"parts":["**Research**\n\n\nExact whitespace"]}},{"id":"a","author":{"role":"assistant"},"status":"finished_successfully","content":{"parts":["```rust\nlet x = 1;\n```"]},"metadata":{"model_slug":"gpt-6"}}]}}"#;
		let data = from_response(body);
		assert_eq!(data.messages.len(), 2);
		assert_eq!(data.messages[0].text, "**Research**\n\n\nExact whitespace");
		assert_eq!(data.messages[1].text, "```rust\nlet x = 1;\n```");
		assert!(data.messages[1].complete);
		assert_eq!(data.identities.len(), 1);
		assert!(!data.identities[0].is_pro());
	}
}
