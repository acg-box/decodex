//! On-demand native evidence, without a second tool-output store.
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{ChiefActivityDetailCursor, ChiefActivityDetailResult};
use serde_json::Value;
#[path = "chief_tool_detail.rs"] mod tool_detail;
#[cfg(test)] use serde_json::json;
use sha2::{Digest as _, Sha256};

#[cfg(test)]
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

pub(crate) async fn read_bound<F, Fut>(
	source: F,
	turn: &str,
	item: &str,
	cursor: Option<&ChiefActivityDetailCursor>,
) -> ChiefActivityDetailResult
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<crate::chief_usage_estimate::Source>>,
{
	let Some(before) = source().await else {
		return ChiefActivityDetailResult::Unavailable;
	};
	let history = tokio::time::timeout(
		std::time::Duration::from_secs(8),
		before.client.thread_read_turn(&before.key.thread, turn),
	)
	.await;
	let Ok(Ok(history)) = history else {
		return ChiefActivityDetailResult::Unavailable;
	};
	let key = &before.key;
	let scope = serde_json::json!([
		key.generation.as_str(),
		key.account.as_str(),
		key.revision,
		key.history_revision,
		key.work,
		key.thread,
		turn,
		item
	])
	.to_string();
	let result = project_text(&history, &key.thread, turn, item)
		.and_then(|text| page(&text, &scope, cursor))
		.unwrap_or(ChiefActivityDetailResult::Unavailable);
	if source().await.is_none_or(|after| after.key != before.key) {
		return ChiefActivityDetailResult::Unavailable;
	}
	result
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

fn project_text(history: &Value, thread: &str, turn: &str, item: &str) -> Option<String> {
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
		"mcpToolCall" | "dynamicToolCall" => parts.extend(tool_detail::parts(item)),
		"functionCallOutput" => parts.extend(crate::chief::timeline::tool_output::parts(item)?),
		"imageView" => {
			parts.push(format!("Execution environment image: {}", item["path"].as_str()?));
			parts.push("The native record does not identify the executor. Image bytes are unavailable through this record.".into());
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
	Some(text)
}

fn project(
	history: &Value,
	thread: &str,
	turn: &str,
	item: &str,
) -> Option<ChiefActivityDetailResult> {
	let text = project_text(history, thread, turn, item)?;
	let limit = 24 * 1024;
	let truncated = text.len() > limit;
	let mut end = text.len().min(limit);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	Some(ChiefActivityDetailResult::Available {
		text: text[..end].into(),
		truncated,
		offset: 0,
		next: None,
	})
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

fn page(
	text: &str,
	scope: &str,
	cursor: Option<&ChiefActivityDetailCursor>,
) -> Option<ChiefActivityDetailResult> {
	let fingerprint = Sha256::digest(serde_json::json!([scope, text]).to_string().as_bytes())
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect::<String>();
	let offset = cursor.map_or(0, |c| c.offset as usize);
	if cursor.is_some_and(|c| c.fingerprint.as_str() != fingerprint || c.offset == 0)
		|| offset >= text.len()
		|| !text.is_char_boundary(offset)
	{
		return None;
	}
	// Eight KiB stays within a public frame even if every byte needs JSON escaping.
	let mut end = (offset + 8 * 1024).min(text.len());
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	let next = (end < text.len())
		.then(|| {
			Some(ChiefActivityDetailCursor {
				offset: u32::try_from(end).ok()?,
				fingerprint: decodex_protocol::WireText::new(fingerprint).ok()?,
			})
		})
		.flatten();
	Some(ChiefActivityDetailResult::Available {
		text: text[offset..end].into(),
		truncated: next.is_some(),
		offset: u32::try_from(offset).ok()?,
		next,
	})
}

#[cfg(test)]
#[path = "chief_tool_detail_tests.rs"]
mod tool_tests;

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
				let ChiefActivityDetailResult::Available { text, truncated, .. } = detail else {
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
		let Some(ChiefActivityDetailResult::Available { text, truncated, .. }) =
			page(&project_text(&history, "t", "u", "i").unwrap(), "scope", None)
		else {
			panic!("detail");
		};
		assert!(truncated);
		assert!(text.len() <= 24 * 1024);
	}
}
