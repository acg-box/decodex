//! Incomplete display recovery without inventing native timeline positions.
use super::{Content, ProjectionError, ordinary};
use crate::chief_usage_estimate::Source;
use decodex_protocol::{ChiefTimelineResult, EntityId};
use serde_json::{Value, json};

pub(super) async fn read(source: &Source) -> Option<ChiefTimelineResult> {
	tokio::time::timeout(std::time::Duration::from_secs(15), async {
		for limit in [100, 50, 25, 10, 1] {
			let value =
				source.client.thread_history_summary(&source.key.thread, limit).await.ok()?;
			let items = match project(&value) {
				Ok(items) => items,
				Err(ProjectionError::Capacity) => continue,
				Err(ProjectionError::Malformed) => return None,
			};
			return Some(ChiefTimelineResult::Summary {
				work_id: EntityId::new(source.key.work.clone()).ok()?,
				account_id: EntityId::new(source.key.account.as_str()).ok()?,
				thread_id: source.key.thread.clone(),
				items,
			});
		}
		None
	})
	.await
	.ok()
	.flatten()
}

fn project(value: &Value) -> Result<Vec<Content>, ProjectionError> {
	let mut items = Vec::new();
	let mut ids = std::collections::HashSet::new();
	for turn in value["turns"].as_array().ok_or(ProjectionError::Malformed)? {
		for item in turn["items"].as_array().ok_or(ProjectionError::Malformed)? {
			if !matches!(item["type"].as_str(), Some("userMessage" | "agentMessage")) {
				continue;
			}
			let content = ordinary(&json!({"turnId":turn["id"],"item":item}))
				.ok_or(ProjectionError::Malformed)?;
			if let Content::Item { turn_id, item_id, .. } = &content
				&& !ids.insert((turn_id.clone(), item_id.clone()))
			{
				return Err(ProjectionError::Malformed);
			}
			items.push(content);
		}
	}
	if serde_json::to_vec(&items).map_err(|_| ProjectionError::Malformed)?.len() > 60 * 1024 {
		return Err(ProjectionError::Capacity);
	}
	Ok(items)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chief_usage_estimate::SourceKey;
	use decodex_codex::app_server_client::AppServerClient;
	use std::sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	};
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn summary_recovery_is_read_only_bounded_and_source_checked() {
		for case in
			["summary", "both_fail", "older", "account", "revision", "history", "process", "thread"]
		{
			let (local, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let stage = Arc::new(AtomicUsize::new(0));
			let observed = stage.clone();
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				let mut methods = Vec::new();
				while let Some(line) = lines.next_line().await.expect("fixture request") {
					let request: Value = serde_json::from_str(&line).expect("request JSON");
					let method = request["method"].as_str().expect("method");
					methods.push(method.to_owned());
					let mut response = match method {
						"thread/read" =>
							json!({"result":{"thread":{"id":"thread","historyMode":"paginated"}}}),
						"thread/timeline/list" =>
							json!({"error":{"code":-32603,"message":"history unavailable"}}),
						"thread/turns/list" => {
							assert_eq!(
								request["params"],
								json!({"threadId":"thread","cursor":null,"limit":100,"sortDirection":"desc","itemsView":"summary"})
							);
							observed.store(1, Ordering::Release);
							if case == "both_fail" {
								json!({"error":{"code":-32603,"message":"summary unavailable"}})
							} else {
								json!({"result":{"data":[
                                {"id":"new","itemsView":"summary","items":[{"id":"answer","type":"agentMessage","text":"Final reply"}]},
                                {"id":"old","itemsView":"summary","items":[{"id":"prompt","type":"userMessage","content":[{"type":"text","text":"Question"}]}]}
                            ],"nextCursor":"must-not-use"}})
							}
						},
						other => panic!("unexpected write or request: {other}"),
					};
					response["id"] = request["id"].clone();
					writer
						.write_all(format!("{response}\n").as_bytes())
						.await
						.expect("fixture reply");
					if method == "thread/turns/list"
						|| (case == "older" && method == "thread/timeline/list")
					{
						break;
					}
				}
				methods
			});
			let result = super::super::read(
				None,
				|| {
					let changed = stage.load(Ordering::Acquire) != 0;
					let client = client.clone();
					async move {
						let mut key = SourceKey {
							work: "work".into(),
							thread: "thread".into(),
							revision: 1,
							history_revision: 0,
							account: decodex_core::AccountId::new(
								"30000000-0000-4000-8000-000000000003",
							)
							.expect("account"),
							generation: decodex_core::ProcessGenerationId::new(
								"10000000-0000-4000-8000-000000000001",
							)
							.expect("generation"),
						};
						if changed {
							match case {
								"account" =>
									key.account = decodex_core::AccountId::new(
										"40000000-0000-4000-8000-000000000004",
									)
									.expect("account"),
								"revision" => key.revision += 1,
								"history" => key.history_revision += 1,
								"process" =>
									key.generation = decodex_core::ProcessGenerationId::new(
										"20000000-0000-4000-8000-000000000002",
									)
									.expect("generation"),
								"thread" => key.thread = "other".into(),
								_ => {},
							}
						}
						Some(Source { key, client })
					}
				},
				(case == "older").then_some("older-cursor"),
			)
			.await;
			let methods = server.await.expect("server");
			if case == "summary" {
				let ChiefTimelineResult::Summary { thread_id, items, .. } = result else {
					panic!("summary expected: {result:?}")
				};
				assert_eq!(thread_id, "thread");
				assert!(
					matches!(&items[0],Content::Item{turn_id,text,..} if turn_id=="old" && text=="Question")
				);
				assert!(
					matches!(&items[1],Content::Item{turn_id,text,..} if turn_id=="new" && text=="Final reply")
				);
				assert_eq!(items.len(), 2);
			} else {
				assert_eq!(result, ChiefTimelineResult::Unavailable);
			}
			assert_eq!(methods.len(), if case == "older" { 2 } else { 4 });
		}
	}
}
