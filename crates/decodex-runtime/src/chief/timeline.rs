//! Bounded public projection of native timeline facts; never enqueue history as input.
use decodex_protocol::{ChiefTimelineContent as Content, ChiefTimelineEntry, ChiefTimelinePage};
use serde_json::{Value, json};
mod attachments;
pub(crate) mod media;
mod metrics;
mod promotions;

pub(crate) async fn read<F, Fut>(
	store: Option<&decodex_database::SqliteStore>,
	source: F,
	cursor: Option<&str>,
) -> decodex_protocol::ChiefTimelineResult
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<crate::chief_usage_estimate::Source>>,
{
	use decodex_codex::app_server_client::ClientError;
	use decodex_protocol::ChiefTimelineResult as Result;
	let Some(before) = source().await else {
		return Result::Unavailable;
	};
	let operation = async {
		for limit in [30, 15, 7, 3, 1] {
			let Some(current) = source().await else {
				return Result::Unavailable;
			};
			if current.key != before.key {
				return Result::Unavailable;
			}
			let response =
				before.client.thread_timeline_page(&before.key.thread, cursor, limit).await;
			let Some(after) = source().await else {
				return Result::Unavailable;
			};
			if after.key != before.key {
				return Result::Unavailable;
			}
			match response {
				Ok(value) => match project(&before.key.thread, &value) {
					Ok(mut page) => {
						promotions::enrich(&before.client, &mut page).await;
						if let Some(store) = store
							&& metrics::enrich(store, &before.key.work, &mut page).await.is_err()
						{
							return Result::Unavailable;
						}
						if source().await.is_none_or(|after| after.key != before.key) {
							return Result::Unavailable;
						}
						if serde_json::to_vec(&page)
							.map_or(true, |encoded| encoded.len() > 60 * 1024)
						{
							continue;
						}
						let (Ok(work_id), Ok(account_id)) = (
							decodex_protocol::EntityId::new(before.key.work.clone()),
							decodex_protocol::EntityId::new(before.key.account.as_str()),
						) else {
							return Result::Unavailable;
						};
						return Result::Available { work_id, account_id, page };
					},
					Err(ProjectionError::Capacity) => {},
					Err(ProjectionError::Malformed) => return Result::Unavailable,
				},
				Err(ClientError::Remote(error)) if error.code == -32601 =>
					return Result::Unsupported,
				Err(ClientError::CapacityExceeded) => {},
				Err(_) => return Result::Unavailable,
			}
		}
		Result::CapacityExceeded
	};
	tokio::time::timeout(std::time::Duration::from_secs(25), operation)
		.await
		.unwrap_or(Result::Unavailable)
}

#[derive(Debug)]
pub(crate) enum ProjectionError {
	Malformed,
	Capacity,
}

pub(crate) fn project(thread: &str, value: &Value) -> Result<ChiefTimelinePage, ProjectionError> {
	let page = project_fields(thread, value).ok_or(ProjectionError::Malformed)?;
	if serde_json::to_vec(&page).map_err(|_| ProjectionError::Malformed)?.len() > 60 * 1024 {
		return Err(ProjectionError::Capacity);
	}
	Ok(page)
}

fn project_fields(thread: &str, value: &Value) -> Option<ChiefTimelinePage> {
	let rows = value["data"].as_array()?;
	if rows.len() > 100 {
		return None;
	}
	let page = ChiefTimelinePage {
		thread_id: id(&Value::String(thread.into()))?,
		entries: rows.iter().map(entry).collect::<Option<Vec<_>>>()?,
		next_cursor: nullable_text(value.get("nextCursor")?, 4096)?,
		active_realtime_session_at_page_start: nullable_text(
			value.get("activeRealtimeSessionAtPageStart")?,
			512,
		)?,
	};
	Some(page)
}

fn entry(row: &Value) -> Option<ChiefTimelineEntry> {
	let content = match row["type"].as_str()? {
		"item" => ordinary(row)?,
		"realtime" => realtime(&row["item"])?,
		"turnStarted" | "turnCompleted" => {
			let completed = row["type"] == "turnCompleted";
			let status = if completed {
				let status = row["status"].as_str()?;
				if !["completed", "interrupted", "failed"].contains(&status) {
					return None;
				}
				Some(status.into())
			} else {
				None
			};
			Content::TurnBoundary {
				turn_id: id(&row["turnId"])?,
				completed,
				status,
				duration_ms: row["durationMs"].as_u64(),
				usage_summary: None,
				error: if completed && !row["error"].is_null() {
					let (message, truncated) = visible_text(row["error"]["message"].as_str()?);
					Some(decodex_protocol::ChiefTimelineError { message, truncated })
				} else {
					None
				},
			}
		},
		_ => return None,
	};
	Some(ChiefTimelineEntry { position: row["position"].as_u64()?, content })
}

fn ordinary(row: &Value) -> Option<Content> {
	let item = &row["item"];
	let kind = id(&item["type"])?;
	let (attachments, omitted) = attachments::project(item);
	let source = match kind.as_str() {
		"agentMessage" => item["text"].as_str()?.to_owned(),
		"userMessage" => {
			let parts = item["content"].as_array()?;
			let mut text = Vec::new();
			for part in parts {
				if part["type"] == "text" {
					text.push(part["text"].as_str()?);
				}
			}
			decodex_protocol::render_chief_async_question_history(&text.join("\n"))
		},
		_ => String::new(),
	};
	let (text, truncated) = visible_text(&source);
	let turn_id = id(&row["turnId"])?;
	let completed = !matches!(item["status"].as_str(), Some("inProgress" | "in_progress"));
	let activity = super::activity::project(&json!({"turnId":turn_id,"item":item}), completed);
	Some(Content::Item {
		turn_id,
		item_id: id(&item["id"])?,
		kind,
		text,
		truncated: truncated || omitted,
		activity,
		attachments,
	})
}

fn realtime(item: &Value) -> Option<Content> {
	let item_id = id(&item["id"])?;
	let session_id = id(&item["realtimeSessionId"])?;
	match item["type"].as_str()? {
		"transcriptSegment" => {
			let role = item["role"].as_str()?;
			if !["user", "assistant"].contains(&role) {
				return None;
			}
			let (text, truncated) = visible_text(item["text"].as_str()?);
			Some(Content::Speech { item_id, session_id, role: role.into(), text, truncated })
		},
		"realtimeSessionStarted" | "realtimeSessionClosed" => {
			let outcome = if item["type"] == "realtimeSessionClosed" {
				let outcome = item["outcome"].as_str()?;
				if !["ended", "failed"].contains(&outcome) {
					return None;
				}
				Some(outcome.into())
			} else {
				None
			};
			Some(Content::VoiceBoundary { item_id, session_id, kind: id(&item["type"])?, outcome })
		},
		"bemItemPromoted" => {
			let presentation = item["presentation"]["type"].as_str()?;
			let index = match presentation {
				"inlineVisualization" =>
					Some(u32::try_from(item["presentation"]["index"].as_u64()?).ok()?),
				"wholeItem" | "inlineMarkdown" => None,
				_ => return None,
			};
			Some(Content::Promotion {
				item_id,
				session_id,
				turn_id: id(&item["turnId"])?,
				agent_item_id: id(&item["itemId"])?,
				presentation: presentation.into(),
				resolved: None,
				index,
			})
		},
		_ => None,
	}
}

fn id(value: &Value) -> Option<String> {
	let value = value.as_str()?;
	(!value.is_empty() && value.len() <= 512).then(|| value.to_owned())
}

fn nullable_text(value: &Value, bound: usize) -> Option<Option<String>> {
	if value.is_null() {
		return Some(None);
	}
	let text = value.as_str()?;
	(!text.is_empty() && text.len() <= bound).then(|| Some(text.into()))
}

fn visible_text(text: &str) -> (String, bool) {
	if decodex_core::contains_credential_material(text) {
		return ("Sensitive details omitted".into(), true);
	}
	let end = text.floor_char_boundary(8192.min(text.len()));
	(text[..end].into(), end < text.len())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn failed_turn_keeps_public_reason_and_explicit_bounds() {
		let mut row = json!({"type":"turnCompleted","position":9,"turnId":"turn",
			"status":"failed","durationMs":5,"error":{"message":"Model is overloaded. Try again.","additionalDetails":"private raw metadata"}});
		let projected = entry(&row).unwrap();
		assert!(matches!(projected.content, Content::TurnBoundary { error:Some(ref error), .. }
			if error.message == "Model is overloaded. Try again." && !error.truncated));
		assert!(!serde_json::to_string(&projected).unwrap().contains("private raw metadata"));
		row["error"]["message"] = json!("界".repeat(4000));
		assert!(
			matches!(entry(&row).unwrap().content, Content::TurnBoundary { error:Some(error), .. }
			if error.truncated && error.message.len() <= 8192 && error.message.chars().all(|c| c == '界'))
		);
		row["error"]["message"] = json!("Bearer abcdefgh");
		assert!(
			matches!(entry(&row).unwrap().content, Content::TurnBoundary { error:Some(error), .. }
			if error.truncated && error.message == "Sensitive details omitted")
		);
		row["error"] = json!({"message":false});
		assert!(entry(&row).is_none());
	}
	#[test]
	fn native_question_replies_share_readable_history_without_decoding_quoted_examples() {
		let envelope = "<send_user_message_question_reply>\n[{\"questionItemId\":\"opaque\",\"question\":\"Which option?\\nExplain why.\",\"answer\":\"Second option.\"}]\n</send_user_message_question_reply>";
		let expected = "> Which option?\n> Explain why.\n\nSecond option.";
		for (kind, source, display) in [
			("userMessage", envelope.to_owned(), expected.to_owned()),
			("userMessage", format!("Example: {envelope}"), format!("Example: {envelope}")),
			("agentMessage", envelope.to_owned(), envelope.to_owned()),
		] {
			let row = json!({"type":"item","position":1,"turnId":"turn","item":{
				"type":kind,"id":"message","text":source,"content":[{"type":"text","text":source}]
			}});
			let content = ordinary(&row).unwrap();
			assert!(
				matches!(content, Content::Item { text, truncated:false, ref turn_id, ref item_id, .. }
				if text == display && turn_id == "turn" && item_id == "message")
			);
		}
	}
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn dense_pages_shrink_without_advancing_cursor_and_source_changes_discard_results() {
		use std::sync::atomic::{AtomicUsize, Ordering};
		for change in ["none", "revision", "account", "generation", "thread", "closed"] {
			let changed = change != "none";
			let (local, remote) = tokio::io::duplex(512 * 1024);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) =
				decodex_codex::app_server_client::AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				let limits = if changed { vec![30] } else { vec![30, 15, 7] };
				for limit in limits {
					for method in ["thread/read", "thread/timeline/list"] {
						let request: Value =
							serde_json::from_str(&lines.next_line().await.unwrap().unwrap())
								.unwrap();
						assert_eq!(request["method"], method);
						let result = if method == "thread/read" {
							json!({"thread":{"id":"thread"}})
						} else {
							assert_eq!(request["params"]["limit"], limit);
							assert_eq!(request["params"]["cursor"], "same-cursor");
							let rows:Vec<_> = (0..limit).map(|n| json!({"type":"realtime","position":n,"item":{"type":"transcriptSegment","id":format!("s{n}"),"realtimeSessionId":"session","role":"user","text":"x".repeat(8192)}})).collect();
							json!({"data":rows,"nextCursor":"older","activeRealtimeSessionAtPageStart":"session"})
						};
						writer
							.write_all(
								format!("{}\n", json!({"id":request["id"],"result":result}))
									.as_bytes(),
							)
							.await
							.unwrap();
					}
				}
			});
			let calls = AtomicUsize::new(0);
			let result = read(
				None,
				|| {
					let observation = calls.fetch_add(1, Ordering::SeqCst);
					let client = client.clone();
					async move {
						if change == "closed" && observation >= 2 {
							return None;
						}
						Some(crate::chief_usage_estimate::Source {
							client,
							key: crate::chief_usage_estimate::SourceKey {
								generation: decodex_core::ProcessGenerationId::new(
									if change == "generation" && observation >= 2 {
										"20000000-0000-4000-8000-000000000002"
									} else {
										"10000000-0000-4000-8000-000000000001"
									},
								)
								.unwrap(),
								account: decodex_core::AccountId::new(
									if change == "account" && observation >= 2 {
										"40000000-0000-4000-8000-000000000004"
									} else {
										"30000000-0000-4000-8000-000000000003"
									},
								)
								.unwrap(),
								revision: if change == "revision" && observation >= 2 {
									2
								} else {
									1
								},
								thread: if change == "thread" && observation >= 2 {
									"other"
								} else {
									"thread"
								}
								.into(),
								work: "work".into(),
							},
						})
					}
				},
				Some("same-cursor"),
			)
			.await;
			server.await.unwrap();
			if changed {
				assert!(matches!(result, decodex_protocol::ChiefTimelineResult::Unavailable));
			} else {
				assert!(
					matches!(result, decodex_protocol::ChiefTimelineResult::Available {page,..} if page.entries.len()==7 && page.next_cursor.as_deref()==Some("older"))
				);
			}
		}
	}

	#[test]
	fn speech_identity_and_promotions_survive_without_copying_tool_arguments() {
		let value = json!({"data":[
			{"type":"realtime","position":1,"item":{"type":"transcriptSegment","id":"speech","realtimeSessionId":"session","role":"assistant","text":"Hello"}},
			{"type":"realtime","position":2,"item":{"type":"bemItemPromoted","id":"promotion","realtimeSessionId":"session","turnId":"turn","itemId":"tool","presentation":{"type":"wholeItem"}}},
			{"type":"item","position":3,"turnId":"turn","item":{"type":"dynamicToolCall","id":"tool","tool":"action","arguments":{"private":"NEVER_PROJECT"},"status":"completed","success":true}}
		],"nextCursor":"older","activeRealtimeSessionAtPageStart":"session"});
		let page = project("thread", &value).unwrap();
		assert_eq!(page.active_realtime_session_at_page_start.as_deref(), Some("session"));
		assert!(
			matches!(&page.entries[1].content, Content::Promotion {item_id,agent_item_id,..} if item_id=="promotion" && agent_item_id=="tool")
		);
		assert!(!serde_json::to_string(&page).unwrap().contains("NEVER_PROJECT"));
	}

	#[test]
	fn malformed_realtime_and_unicode_bounds_are_explicit() {
		let (text, truncated) = visible_text(&"界".repeat(4000));
		assert!(truncated && text.len() <= 8192 && text.ends_with('界'));
		let item = json!({"type":"bemItemPromoted","id":"p","realtimeSessionId":"s","turnId":"t","itemId":"i","presentation":{"type":"inlineVisualization","index":-1}});
		assert!(realtime(&item).is_none());
		assert!(project("thread", &json!({"data":[],"nextCursor":null})).is_err());
	}

	#[test]
	fn large_pages_report_capacity_and_running_items_stay_running() {
		let rows: Vec<_> = (0..20).map(|n| json!({"type":"realtime","position":n,
			"item":{"type":"transcriptSegment","id":format!("speech-{n}"),"realtimeSessionId":"session","role":"user","text":"x".repeat(8192)}})).collect();
		assert!(matches!(
			project(
				"thread",
				&json!({"data":rows,"nextCursor":null,"activeRealtimeSessionAtPageStart":null})
			),
			Err(ProjectionError::Capacity)
		));
		let content = ordinary(&json!({"turnId":"turn","item":{"type":"commandExecution","id":"command","status":"inProgress"}})).unwrap();
		assert!(
			matches!(content, Content::Item { activity:Some(activity), .. } if activity.status=="running")
		);
	}
}
