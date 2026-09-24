//! On-demand native evidence, without a second tool-output store.
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::ChiefActivityDetailResult;
use serde_json::Value;
#[cfg(test)] use serde_json::json;

pub(crate) async fn read(
	client: &AppServerClient,
	thread: &str,
	turn: &str,
	item: &str,
) -> ChiefActivityDetailResult {
	let result = tokio::time::timeout(
		std::time::Duration::from_secs(8),
		client.thread_read_turn(thread, turn),
	)
	.await;
	let Ok(Ok(history)) = result else {
		return ChiefActivityDetailResult::Unavailable;
	};
	project(&history, thread, turn, item).unwrap_or(ChiefActivityDetailResult::Unavailable)
}

pub(crate) async fn read_file_changes(
	client: &AppServerClient,
	thread: &str,
	turn: &str,
	item: &str,
) -> ChiefActivityDetailResult {
	let Ok(Ok(history)) = tokio::time::timeout(
		std::time::Duration::from_secs(8),
		client.thread_read_turn(thread, turn),
	)
	.await
	else {
		return ChiefActivityDetailResult::Unavailable;
	};
	let matches_file = history
		.pointer("/thread/turns")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter(|entry| entry["id"].as_str() == Some(turn))
		.flat_map(|entry| entry["items"].as_array().into_iter().flatten())
		.any(|entry| entry["id"].as_str() == Some(item) && entry["type"] == "fileChange");
	if !matches_file {
		return ChiefActivityDetailResult::Unavailable;
	}
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
					parts.push(format!("Path: {path}"));
				}
				if let Some(kind) = change.pointer("/kind/type").and_then(Value::as_str) {
					parts.push(format!("Change: {kind}"));
				}
				if let Some(path) = change.pointer("/kind/move_path").and_then(Value::as_str) {
					parts.push(format!("Move destination: {path}"));
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
		"webSearch" => parts.extend(web_details(item)),
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

fn web_details(item: &Value) -> Vec<String> {
	let action = &item["action"];
	let mut parts = Vec::new();
	match action["type"].as_str() {
		Some("search") => {
			if let Some(query) = action["query"].as_str().filter(|text| !text.is_empty()) {
				parts.push(query.to_owned());
			}
			for query in action["queries"]
				.as_array()
				.into_iter()
				.flatten()
				.filter_map(Value::as_str)
				.filter(|text| !text.is_empty())
			{
				if !parts.iter().any(|part| part == query) {
					parts.push(query.to_owned());
				}
			}
		},
		Some("openPage" | "findInPage") =>
			for field in ["url", "pattern"] {
				if let Some(text) = action[field].as_str().filter(|text| !text.is_empty()) {
					parts.push(text.to_owned());
				}
			},
		_ => {},
	}
	if parts.is_empty()
		&& let Some(query) = item["query"].as_str().filter(|text| !text.is_empty())
	{
		parts.push(query.to_owned());
	}
	match item["results"].as_array() {
		Some(results) if results.is_empty() => parts.push("No results returned.".into()),
		Some(results) => {
			// Native result objects are intentionally extensible. Keep all fields,
			// including errors, and apply the common redaction before display limits.
			for result in results {
				parts.push(result.to_string());
			}
		},
		None => parts.push("Results not reported.".into()),
	}
	parts
}

#[cfg(test)]
#[path = "chief_web_detail_tests.rs"]
mod web_tests;

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
	#[tokio::test]
	async fn file_approval_loads_exact_paginated_item_without_full_history() {
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		for kind in ["fileChange", "commandExecution"] {
			let (local, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				for (method, result) in [
					("thread/read", json!({"thread":{"id":"thread","historyMode":"paginated"}})),
					("thread/turns/list", json!({"data":[{"id":"turn"}],"nextCursor":null})),
					(
						"thread/items/list",
						json!({"data":[{"turnId":"turn","item":{"id":"patch","type":kind,"command":"must not show command", "changes":[{"path":"C:\\remote\\old.txt","kind":{"type":"update","move_path":"C:\\remote\\new.txt"},"diff":"-old\n+new"}]}}],"nextCursor":null}),
					),
				] {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
					assert_eq!(request["method"], method);
					assert_ne!(request["params"]["includeTurns"], true);
					writer
						.write_all(
							format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
						)
						.await
						.unwrap();
				}
			});
			let detail = read_file_changes(&client, "thread", "turn", "patch").await;
			server.await.unwrap();
			if kind == "fileChange" {
				let ChiefActivityDetailResult::Available { text, truncated } = detail else {
					panic!("file detail");
				};
				assert!(text.contains("old.txt"));
				assert!(text.contains("Move destination: C:"));
				assert!(text.contains("new.txt"));
				assert!(text.contains("-old\n+new"));
				assert!(!text.contains("must not show"));
				assert!(!truncated);
			} else {
				assert_eq!(detail, ChiefActivityDetailResult::Unavailable);
			}
		}
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
