//! On-demand native evidence, without a second tool-output store.
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::ChiefActivityDetailResult;
use serde_json::Value;
#[cfg(test)] use serde_json::json;

pub(crate) async fn read_bound<F, Fut>(
	source: F,
	turn: &str,
	item: &str,
) -> ChiefActivityDetailResult
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<crate::chief_usage_estimate::Source>>,
{
	let Some(before) = source().await else {
		return ChiefActivityDetailResult::Unavailable;
	};
	let result = read(&before.client, &before.key.thread, turn, item).await;
	if source().await.is_none_or(|after| after.key != before.key) {
		return ChiefActivityDetailResult::Unavailable;
	}
	result
}

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
			append_app_context(item, &mut parts);
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

fn append_app_context(item: &Value, parts: &mut Vec<String>) {
	if item["type"] != "mcpToolCall" {
		return;
	}
	// Native invocation metadata owns account selection. An arbitrary link_id
	// argument can describe a resource rather than the connected account.
	for (field, label) in [
		("appName", "App"),
		("connectorId", "Connector"),
		("linkId", "Connected account link"),
		("actionName", "Action"),
	] {
		if let Some(value) = item["appContext"][field].as_str().filter(|value| !value.is_empty()) {
			parts.push(format!("{label}: {value}"));
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn tool_detail_uses_native_account_identity_without_inferring_from_arguments() {
		for (kind, context, expected) in [
			(
				"mcpToolCall",
				json!({"appName":"Calendar","connectorId":"calendar","linkId":" work/link ","actionName":"Create event"}),
				Some(" work/link "),
			),
			(
				"mcpToolCall",
				json!({"connectorId":"calendar","linkId":"personal/link"}),
				Some("personal/link"),
			),
			("mcpToolCall", Value::Null, None),
			("dynamicToolCall", json!({"linkId":"unrelated"}), None),
		] {
			let history = json!({"thread":{"id":"t","turns":[{"id":"u","items":[{"id":"i","type":kind,"server":"codex_apps","tool":"create","appContext":context,"arguments":{"link_id":"argument-must-not-authorize"}}]}]}});
			let Some(ChiefActivityDetailResult::Available { text, truncated }) =
				project(&history, "t", "u", "i")
			else {
				panic!("tool detail");
			};
			assert!(!text.contains("argument-must-not-authorize"));
			assert!(!truncated);
			match expected {
				Some(link) => assert!(text.contains(&format!("Connected account link: {link}"))),
				None => assert!(!text.contains("Connected account link:")),
			}
		}
	}
	#[tokio::test]
	async fn activity_detail_rejects_changed_or_missing_source() {
		use crate::chief_usage_estimate::{Source, SourceKey};
		use decodex_core::{AccountId, ProcessGenerationId};
		use std::sync::atomic::{AtomicUsize, Ordering};
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		for change in [
			"none", "account", "process", "revision", "history", "thread", "work", "closed",
			"absent",
		] {
			let (local, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				if change == "absent" {
					assert!(lines.next_line().await.unwrap().is_none());
					return;
				}
				for (method, result) in [
					("thread/read", json!({"thread":{"id":"thread","historyMode":"paginated"}})),
					("thread/turns/list", json!({"data":[{"id":"turn"}],"nextCursor":null})),
					(
						"thread/items/list",
						json!({"data":[{"turnId":"turn","item":{"id":"item","type":"commandExecution","aggregatedOutput":"Passed"}}],"nextCursor":null}),
					),
				] {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
					assert_eq!(request["method"], method);
					assert_eq!(request["params"]["threadId"], "thread");
					writer
						.write_all(
							format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
						)
						.await
						.unwrap();
				}
			});
			let calls = AtomicUsize::new(0);
			let result = read_bound(
				|| {
					let later = calls.fetch_add(1, Ordering::SeqCst) > 0;
					let client = client.clone();
					async move {
						if change == "absent" || (later && change == "closed") {
							return None;
						}
						Some(Source {
							client,
							key: SourceKey {
								generation: ProcessGenerationId::new(
									if later && change == "process" {
										"20000000-0000-4000-8000-000000000002"
									} else {
										"10000000-0000-4000-8000-000000000001"
									},
								)
								.unwrap(),
								account: AccountId::new(if later && change == "account" {
									"40000000-0000-4000-8000-000000000004"
								} else {
									"30000000-0000-4000-8000-000000000003"
								})
								.unwrap(),
								revision: i64::from(later && change == "revision"),
								history_revision: u64::from(later && change == "history"),
								thread: if later && change == "thread" {
									"other"
								} else {
									"thread"
								}
								.into(),
								work: if later && change == "work" { "other" } else { "work" }
									.into(),
							},
						})
					}
				},
				"turn",
				"item",
			)
			.await;
			if change == "none" {
				assert!(
					matches!(result, ChiefActivityDetailResult::Available { text, .. } if text == "Passed")
				);
			} else {
				assert_eq!(result, ChiefActivityDetailResult::Unavailable, "{change}");
			}
			assert_eq!(calls.load(Ordering::SeqCst), if change == "absent" { 1 } else { 2 });
			drop(client);
			server.await.unwrap();
		}
	}
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
