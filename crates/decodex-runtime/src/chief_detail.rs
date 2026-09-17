//! On-demand native evidence, without a second tool-output store.
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::ChiefActivityDetailResult;
use serde_json::{Value, json};

pub(crate) async fn read(
	client: &AppServerClient,
	thread: &str,
	turn: &str,
	item: &str,
) -> ChiefActivityDetailResult {
	let result = tokio::time::timeout(
		std::time::Duration::from_secs(8),
		client.thread_read(json!({"threadId":thread,"includeTurns":true})),
	)
	.await;
	let Ok(Ok(history)) = result else {
		return ChiefActivityDetailResult::Unavailable;
	};
	project(&history, thread, turn, item).unwrap_or(ChiefActivityDetailResult::Unavailable)
}

fn project(
	history: &Value,
	thread: &str,
	turn: &str,
	item: &str,
) -> Option<ChiefActivityDetailResult> {
	if history.pointer("/thread/id")?.as_str()? != thread {
		return None;
	}
	let turn =
		history.pointer("/thread/turns")?.as_array()?.iter().find(|entry| entry["id"] == turn)?;
	let item = turn["items"].as_array()?.iter().find(|entry| entry["id"] == item)?;
	let mut parts = Vec::new();
	match item["type"].as_str()? {
		"commandExecution" => {
			for field in ["command", "cwd", "aggregatedOutput"] {
				if let Some(text) = item[field].as_str() {
					parts.push(text.to_owned());
				}
			}
			if let Some(code) = item["exitCode"].as_i64() {
				parts.push(format!("Exit code: {code}"));
			}
		},
		"fileChange" =>
			for change in item["changes"].as_array()? {
				if let Some(path) = change["path"].as_str() {
					parts.push(path.into());
				}
				if let Some(diff) = change["diff"].as_str() {
					parts.push(diff.into());
				}
			},
		"mcpToolCall" | "dynamicToolCall" => {
			for field in ["server", "tool"] {
				if let Some(text) = item[field].as_str() {
					parts.push(text.into());
				}
			}
			let content = item.pointer("/result/content").or_else(|| item.get("contentItems"));
			for part in content.and_then(Value::as_array).into_iter().flatten() {
				if matches!(part["type"].as_str(), Some("text" | "inputText"))
					&& let Some(text) = part["text"].as_str()
				{
					parts.push(text.into());
				}
			}
			if let Some(message) = item.pointer("/error/message").and_then(Value::as_str) {
				parts.push(message.into());
			}
		},
		"webSearch" =>
			if let Some(query) = item["query"].as_str() {
				parts.push(query.into());
			},
		_ => return None,
	}
	let text = parts
		.into_iter()
		.map(|part| {
			if decodex_core::contains_credential_material(&part) {
				"[Sensitive content omitted]".into()
			} else {
				part
			}
		})
		.collect::<Vec<String>>()
		.join("\n\n");
	if text.is_empty() {
		return None;
	}
	let limit = 24 * 1024;
	let truncated = text.len() > limit;
	let mut end = text.len().min(limit);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	Some(ChiefActivityDetailResult::Available { text: text[..end].into(), truncated })
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn exact_source_required_and_reasoning_not_projected() {
		let history = json!({"thread":{"id":"thread","turns":[{"id":"turn","items":[{"id":"item","type":"commandExecution","command":"cargo test","aggregatedOutput":"Passed","exitCode":0},{"id":"private","type":"reasoning","text":"private"}]}]}});
		assert!(
			matches!(project(&history,"thread","turn","item"),Some(ChiefActivityDetailResult::Available {text,..}) if text.contains("Passed"))
		);
		assert!(project(&history, "other", "turn", "item").is_none());
		assert!(project(&history, "thread", "other", "item").is_none());
		assert!(project(&history, "thread", "turn", "private").is_none());
	}
	#[test]
	fn output_is_bounded_at_utf8_boundary() {
		let history = json!({"thread":{"id":"t","turns":[{"id":"u","items":[{"id":"i","type":"commandExecution","aggregatedOutput":"界".repeat(10000)}]}]}});
		let Some(ChiefActivityDetailResult::Available { text, truncated }) =
			project(&history, "t", "u", "i")
		else {
			panic!("detail");
		};
		assert!(truncated);
		assert!(text.len() <= 24 * 1024);
	}
}
