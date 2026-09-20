//! Exact saved usage supplements native timeline boundaries; it is not native timeline data.
use decodex_protocol::{ChiefTimelineContent as Content, ChiefTimelinePage, ChiefTurnUsageDto};

pub(super) async fn enrich(
	store: &decodex_database::SqliteStore,
	work: &str,
	page: &mut ChiefTimelinePage,
) -> Result<(), decodex_database::StoreError> {
	let turns = page
		.entries
		.iter()
		.filter_map(|entry| match &entry.content {
			Content::TurnBoundary { turn_id, completed: true, .. } => Some(turn_id.clone()),
			_ => None,
		})
		.collect::<Vec<_>>();
	if turns.is_empty() {
		return Ok(());
	}
	let saved = store.read_chief_turn_metrics(work.into(), page.thread_id.clone(), turns).await?;
	for entry in &mut page.entries {
		if let Content::TurnBoundary { turn_id, completed: true, usage_summary, .. } =
			&mut entry.content
		{
			*usage_summary = saved.iter().find(|saved| &saved.turn_id == turn_id).and_then(summary);
		}
	}
	Ok(())
}

fn summary(saved: &decodex_database::ChiefTurnMetrics) -> Option<String> {
	let mut parts = Vec::new();
	if let Some(usage) = saved
		.usage_json
		.as_deref()
		.and_then(|value| serde_json::from_str::<ChiefTurnUsageDto>(value).ok())
		&& usage.input_tokens <= i64::MAX as u64
		&& usage.output_tokens <= i64::MAX as u64
	{
		parts.push(format!(
			"Turn tokens: input {}, output {}.",
			usage.input_tokens, usage.output_tokens
		));
	}
	if let Some(observation) = saved
		.observation_json
		.as_deref()
		.and_then(|value| serde_json::from_str(value).ok())
		.and_then(|value| super::super::observations::usage_text(&value))
	{
		parts.push(observation);
	}
	(!parts.is_empty()).then(|| parts.join("\n"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[test]
	fn whole_turn_delta_and_last_response_observation_keep_distinct_labels() {
		let counts = json!({"totalTokens":150,"inputTokens":120,"cachedInputTokens":50,"cacheWriteInputTokens":10,"outputTokens":30,"reasoningOutputTokens":8});
		let mut saved = decodex_database::ChiefTurnMetrics {
			turn_id: "turn".into(),
			usage_json: Some(json!({"input_tokens":240,"output_tokens":60}).to_string()),
			observation_json: Some(
				json!({"last":counts,"total":{"totalTokens":900,"inputTokens":700,"cachedInputTokens":200,"cacheWriteInputTokens":20,"outputTokens":200,"reasoningOutputTokens":100},"modelContextWindow":128000}).to_string(),
			),
		};
		let text = summary(&saved).unwrap();
		assert!(text.contains("Turn tokens: input 240, output 60."));
		assert!(text.contains(
			"Last response tokens: input 120, cached input 50, output 30, reasoning output 8."
		));
		assert!(text.contains("Cache write input tokens: 10."));
		assert!(text.contains("Thread total tokens: 900."));
		saved.usage_json = None;
		assert!(!summary(&saved).unwrap().contains("Turn tokens:"));
		saved.observation_json = None;
		assert_eq!(summary(&saved), None);
		saved.usage_json = Some(json!({"input_tokens":0,"output_tokens":0}).to_string());
		assert_eq!(summary(&saved).as_deref(), Some("Turn tokens: input 0, output 0."));
		saved.usage_json = Some(json!({"input_tokens":u64::MAX,"output_tokens":0}).to_string());
		assert_eq!(summary(&saved), None);
	}
	#[tokio::test]
	async fn native_page_reads_enrich_exact_turns_and_recheck_source_after_saved_usage() {
		use crate::chief_usage_estimate::{Source, SourceKey};
		use decodex_codex::app_server_client::AppServerClient;
		use decodex_core::{AccountId, DecodexRoot, ProcessGenerationId};
		use decodex_database::{
			ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus, EnqueueChiefEvent,
			SqliteStore,
		};
		use std::sync::atomic::{AtomicUsize, Ordering};
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		store
			.create_chief_work_item(ChiefWorkItem {
				id: "work".into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: "Work".into(),
				instructions: "Task".into(),
				codex_thread_id: None,
				dispatch_state: ChiefDispatchState::Idle,
				active_turn_id: None,
				status: ChiefWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store.enqueue_chief_event(EnqueueChiefEvent {source_event_id:json!(["turn/completed","thread","turn"]).to_string(),work_item_id:"work".into(),event_kind:"chief_turn_completed".into(),payload:json!({"terminal":{"threadId":"thread","turn":{"id":"turn"}},"usage":{"input_tokens":11,"output_tokens":2}}).to_string()}).await.unwrap();
		for change in [false, true] {
			let (local, remote) = tokio::io::duplex(8192);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				for (method, result) in [
					("thread/read", json!({"thread":{"id":"thread"}})),
					(
						"thread/timeline/list",
						json!({"data":[{"type":"turnCompleted","position":1,"turnId":"turn","status":"completed","error":null},{"type":"turnCompleted","position":2,"turnId":"unreported","status":"completed","error":null}],"nextCursor":null,"activeRealtimeSessionAtPageStart":null}),
					),
				] {
					let request: serde_json::Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
					assert_eq!(request["method"], method);
					writer
						.write_all(
							format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
						)
						.await
						.unwrap();
				}
			});
			let calls = AtomicUsize::new(0);
			let result = super::super::read(
				Some(&store),
				|| {
					let after = calls.fetch_add(1, Ordering::SeqCst) >= 3;
					std::future::ready(Some(Source {
						client: client.clone(),
						key: SourceKey {
							history_revision: 0,
							generation: ProcessGenerationId::new(
								"10000000-0000-4000-8000-000000000001",
							)
							.unwrap(),
							account: AccountId::new("30000000-0000-4000-8000-000000000003")
								.unwrap(),
							revision: if change && after { 2 } else { 1 },
							thread: "thread".into(),
							work: "work".into(),
						},
					}))
				},
				None,
			)
			.await;
			server.await.unwrap();
			if change {
				assert_eq!(result, decodex_protocol::ChiefTimelineResult::Unavailable);
			} else {
				let decodex_protocol::ChiefTimelineResult::Available { page, .. } = result else {
					panic!("available native page")
				};
				assert!(
					matches!(&page.entries[0].content,Content::TurnBoundary {usage_summary:Some(text),..} if text=="Turn tokens: input 11, output 2.")
				);
				assert!(matches!(
					&page.entries[1].content,
					Content::TurnBoundary { usage_summary: None, .. }
				));
			}
		}
	}
}
