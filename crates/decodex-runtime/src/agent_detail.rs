//! On-demand native evidence, without a second tool-output store.
#[path = "agent_tool_detail.rs"] mod tool_detail;

use std::{future::Future, time::Duration};

use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::time;

use crate::{
	agent::{image_generation, timeline::tool_output},
	agent_usage_estimate::Source,
};
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::MAX_NATIVE_MESSAGE_BYTES;
use decodex_protocol::{AgentActivityDetailCursor, AgentActivityDetailResult, WireText};

pub(crate) fn saved_file_changes(payload: &Value) -> Option<String> {
	let params = &payload["params"];
	let item = &payload["fileChange"];

	if payload["method"] != "item/fileChange/requestApproval"
		|| item["type"] != "fileChange"
		|| item["id"] != params["itemId"]
	{
		return None;
	}

	let (thread, turn, id) =
		(params["threadId"].as_str()?, params["turnId"].as_str()?, params["itemId"].as_str()?);
	let history = serde_json::json!({"thread":{"id":thread,"turns":[{"id":turn,"items":[item]}]}});

	project_text(&history, thread, turn, id)
}

#[cfg(test)]
pub(crate) async fn read(
	client: &AppServerClient,
	thread: &str,
	turn: &str,
	item: &str,
) -> AgentActivityDetailResult {
	let result = tokio::time::timeout(
		std::time::Duration::from_secs(8),
		client.thread_read_turn(thread, turn),
	)
	.await;
	let Ok(Ok(history)) = result else {
		return AgentActivityDetailResult::Unavailable;
	};

	project(&history, thread, turn, item).unwrap_or(AgentActivityDetailResult::Unavailable)
}

pub(crate) async fn read_bound<F, Fut>(
	source: F,
	turn: &str,
	item: &str,
	cursor: Option<&AgentActivityDetailCursor>,
) -> AgentActivityDetailResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return AgentActivityDetailResult::Unavailable;
	};
	let history = time::timeout(
		Duration::from_secs(8),
		before.client.thread_read_turn(&before.key.thread, turn),
	)
	.await;
	let Ok(Ok(history)) = history else {
		return AgentActivityDetailResult::Unavailable;
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
		.unwrap_or(AgentActivityDetailResult::Unavailable);

	if source().await.is_none_or(|after| after.key != before.key) {
		return AgentActivityDetailResult::Unavailable;
	}

	result
}

pub(crate) async fn read_file_changes(
	client: &AppServerClient,
	thread: &str,
	turn: &str,
	item: &str,
) -> AgentActivityDetailResult {
	let Ok(Ok(history)) =
		time::timeout(Duration::from_secs(8), client.thread_read_turn(thread, turn)).await
	else {
		return AgentActivityDetailResult::Unavailable;
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
		return AgentActivityDetailResult::Unavailable;
	}

	project(&history, thread, turn, item).unwrap_or(AgentActivityDetailResult::Unavailable)
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
		"functionCallOutput" => parts.extend(tool_output::parts(item)?),
		"imageGeneration" => {
			parts.push(format!("Image generation: {}", item["status"].as_str()?));

			if let Some(detail) = image_generation::quota_detail(item) {
				parts.push(detail);
			}
		},
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
) -> Option<AgentActivityDetailResult> {
	let text = project_text(history, thread, turn, item)?;

	if text.len() > MAX_NATIVE_MESSAGE_BYTES {
		return None;
	}

	Some(AgentActivityDetailResult::Available { text, truncated: false, offset: 0, next: None })
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
	cursor: Option<&AgentActivityDetailCursor>,
) -> Option<AgentActivityDetailResult> {
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
	let mut end = (offset + 8 * 1_024).min(text.len());

	while !text.is_char_boundary(end) {
		end -= 1;
	}

	let next = (end < text.len())
		.then(|| {
			Some(AgentActivityDetailCursor {
				offset: u32::try_from(end).ok()?,
				fingerprint: WireText::new(fingerprint).ok()?,
			})
		})
		.flatten();

	Some(AgentActivityDetailResult::Available {
		text: text[offset..end].into(),
		truncated: next.is_some(),
		offset: u32::try_from(offset).ok()?,
		next,
	})
}

#[cfg(test)]
#[path = "agent_tool_detail_tests.rs"]
mod tool_tests;
#[cfg(test)]
#[path = "agent_web_detail_tests.rs"]
mod web_tests;
#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicUsize, Ordering};

	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use crate::{
		agent::timeline,
		agent_detail::{self, AgentActivityDetailResult, AppServerClient, Value},
		agent_usage_estimate::{Source, SourceKey},
	};
	use decodex_core::{AccountId, ProcessGenerationId};

	#[test]
	fn image_quota_is_visible_in_timeline_and_exact_activity_details() {
		let item = serde_json::json!({"id":"image","type":"imageGeneration","status":"failed","failure":{"type":"usageLimitExceeded","limitId":"image_gen","resetsAt":1_790_683_200},"result":""});
		let page=timeline::project("thread",&serde_json::json!({"data":[{"type":"item","turnId":"turn","position":0,"item":item}],"nextCursor":null,"activeRealtimeSessionAtPageStart":null})).unwrap();
		let decodex_protocol::AgentTimelineContent::Item { activity: Some(activity), .. } =
			&page.entries[0].content
		else {
			panic!("image activity")
		};

		assert_eq!(activity.label, "Image generation limit reached");
		assert_eq!(activity.status, "failed");

		let history =
			serde_json::json!({"thread":{"id":"thread","turns":[{"id":"turn","items":[item]}]}});
		let text = agent_detail::project_text(&history, "thread", "turn", "image").unwrap();

		assert!(text.contains("Image generation usage limit reached"));
		assert!(text.contains("2026-09-29T12:00:00Z"));
		assert!(agent_detail::project_text(&history, "other", "turn", "image").is_none());
	}

	#[test]
	fn exact_source_required_and_reasoning_not_projected() {
		let history = serde_json::json!({"thread":{"id":"thread","turns":[{"id":"turn","items":[{"id":"item","type":"commandExecution","command":"cargo test","aggregatedOutput":"Passed","exitCode":0},{"id":"private","type":"reasoning","text":"private"}]}]}});

		assert!(
			matches!(agent_detail::project(&history,"thread","turn","item"),Some(AgentActivityDetailResult::Available {text,..}) if text.contains("Passed"))
		);
		assert!(agent_detail::project(&history, "other", "turn", "item").is_none());
		assert!(agent_detail::project(&history, "thread", "other", "item").is_none());
		assert!(agent_detail::project(&history, "thread", "turn", "private").is_none());
	}

	#[test]
	fn output_is_bounded_at_utf8_boundary() {
		let history = serde_json::json!({"thread":{"id":"t","turns":[{"id":"u","items":[{"id":"i","type":"commandExecution","aggregatedOutput":"界".repeat(10_000)}]}]}});
		let Some(AgentActivityDetailResult::Available { text, truncated, offset, next }) =
			agent_detail::page(
				&agent_detail::project_text(&history, "t", "u", "i").unwrap(),
				"scope",
				None,
			)
		else {
			panic!("detail");
		};

		assert!(truncated);
		assert!(text.len() <= 8 * 1_024);
		assert_eq!(text, "界".repeat(2_730));
		assert_eq!(offset, 0);
		assert_eq!(next.unwrap().offset, 8_190);
	}

	#[test]
	fn complete_detail_is_not_shortened_before_request_paging() {
		let history = serde_json::json!({"thread":{"id":"t","turns":[{"id":"u","items":[{"id":"i","type":"commandExecution","aggregatedOutput":"界".repeat(10_000)}]}]}});
		let Some(AgentActivityDetailResult::Available { text, truncated, .. }) =
			agent_detail::project(&history, "t", "u", "i")
		else {
			panic!("detail");
		};

		assert!(!truncated);
		assert_eq!(text, "界".repeat(10_000));
	}

	#[tokio::test]
	async fn file_approval_loads_exact_paginated_item_without_full_history() {
		for kind in ["fileChange", "commandExecution"] {
			let (local, remote) = io::duplex(65_536);
			let (reader, writer) = io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = io::split(remote);
				let mut lines = BufReader::new(reader).lines();

				for (method, result) in [
					(
						"thread/read",
						serde_json::json!({"thread":{"id":"thread","historyMode":"paginated"}}),
					),
					(
						"thread/turns/list",
						serde_json::json!({"data":[{"id":"turn"}],"nextCursor":null}),
					),
					(
						"thread/items/list",
						serde_json::json!({"data":[{"turnId":"turn","item":{"id":"patch","type":kind,"command":"must not show command", "changes":[{"path":"C:\\remote\\old.txt","kind":{"type":"update","move_path":"C:\\remote\\new.txt"},"diff":format!("-old\n+new{} REQUIRED PATCH SUFFIX", "界".repeat(20_000))}]}}],"nextCursor":null}),
					),
				] {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

					assert_eq!(request["method"], method);
					assert_ne!(request["params"]["includeTurns"], true);

					writer
						.write_all(
							format!(
								"{}\n",
								serde_json::json!({"id":request["id"],"result":result})
							)
							.as_bytes(),
						)
						.await
						.unwrap();
				}
			});
			let detail = agent_detail::read_file_changes(&client, "thread", "turn", "patch").await;

			server.await.unwrap();

			if kind == "fileChange" {
				let AgentActivityDetailResult::Available { text, truncated, .. } = detail else {
					panic!("file detail");
				};

				assert!(text.contains("old.txt"));
				assert!(text.contains("Move destination: C:"));
				assert!(text.contains("new.txt"));
				assert!(text.contains("-old\n+new"));
				assert!(text.ends_with("REQUIRED PATCH SUFFIX"));
				assert!(text.len() > 24 * 1_024);
				assert!(!text.contains("must not show"));
				assert!(!truncated);
			} else {
				assert_eq!(detail, AgentActivityDetailResult::Unavailable);
			}
		}
	}

	#[tokio::test]
	async fn activity_detail_rejects_changed_or_missing_source() {
		for change in [
			"none", "account", "process", "revision", "history", "thread", "work", "closed",
			"absent",
		] {
			let (local, remote) = io::duplex(65_536);
			let (reader, writer) = io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = io::split(remote);
				let mut lines = BufReader::new(reader).lines();

				if change == "absent" {
					assert!(lines.next_line().await.unwrap().is_none());

					return;
				}

				for (method, result) in [
					(
						"thread/read",
						serde_json::json!({"thread":{"id":"thread","historyMode":"paginated"}}),
					),
					(
						"thread/turns/list",
						serde_json::json!({"data":[{"id":"turn"}],"nextCursor":null}),
					),
					(
						"thread/items/list",
						serde_json::json!({"data":[{"turnId":"turn","item":{"id":"item","type":"commandExecution","aggregatedOutput":"Passed"}}],"nextCursor":null}),
					),
				] {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

					assert_eq!(request["method"], method);
					assert_eq!(request["params"]["threadId"], "thread");

					writer
						.write_all(
							format!(
								"{}\n",
								serde_json::json!({"id":request["id"],"result":result})
							)
							.as_bytes(),
						)
						.await
						.unwrap();
				}
			});
			let calls = AtomicUsize::new(0);
			let result = agent_detail::read_bound(
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
				None,
			)
			.await;

			if change == "none" {
				assert!(
					matches!(result, AgentActivityDetailResult::Available { text, .. } if text == "Passed")
				);
			} else {
				assert_eq!(result, AgentActivityDetailResult::Unavailable, "{change}");
			}

			assert_eq!(calls.load(Ordering::SeqCst), if change == "absent" { 1 } else { 2 });

			drop(client);

			server.await.unwrap();
		}
	}
}
