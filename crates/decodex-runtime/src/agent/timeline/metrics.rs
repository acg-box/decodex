//! Exact saved usage supplements native timeline boundaries; it is not native timeline data.
use crate::agent::observations;
use decodex_codex::ThreadTokenUsage;
use decodex_database::{AgentResponseUsageSummary, AgentTurnMetrics, SqliteStore, StoreError};
use decodex_protocol::{AgentTimelineContent, AgentTimelinePage, AgentTurnUsageDto};

pub(crate) async fn enrich(
	store: &SqliteStore,
	work: &str,
	page: &mut AgentTimelinePage,
) -> Result<(), StoreError> {
	let items = page
		.entries
		.iter()
		.filter_map(|entry| match &entry.content {
			AgentTimelineContent::Item { turn_id, item_id, activity: Some(activity), .. }
				if activity.duration_ms.is_none() && activity.status != "running" =>
				Some((turn_id.clone(), item_id.clone())),
			_ => None,
		})
		.collect::<Vec<_>>();

	if !items.is_empty() {
		let durations =
			store.read_agent_activity_durations(work.into(), page.thread_id.clone(), items).await?;

		for entry in &mut page.entries {
			if let AgentTimelineContent::Item { turn_id, item_id, activity: Some(activity), .. } =
				&mut entry.content
				&& activity.duration_ms.is_none()
				&& activity.status != "running"
				&& let Some((_, _, duration)) =
					durations.iter().find(|(turn, item, _)| turn == turn_id && item == item_id)
			{
				activity.duration_ms = Some(*duration);
			}
		}
	}

	let turns = page
		.entries
		.iter()
		.filter_map(|entry| match &entry.content {
			AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } =>
				Some(turn_id.clone()),
			_ => None,
		})
		.collect::<Vec<_>>();

	if turns.is_empty() {
		return Ok(());
	}

	let responses =
		store.read_agent_response_usage(work.into(), page.thread_id.clone(), turns.clone()).await?;
	let saved = store.read_agent_turn_metrics(work.into(), page.thread_id.clone(), turns).await?;

	for entry in &mut page.entries {
		if let AgentTimelineContent::TurnBoundary {
			turn_id,
			completed: true,
			usage_summary,
			usage,
			..
		} = &mut entry.content
		{
			*usage = saved
				.iter()
				.find(|saved| &saved.turn_id == turn_id)
				.and_then(|saved| saved.usage_json.as_deref())
				.and_then(|value| serde_json::from_str::<AgentTurnUsageDto>(value).ok())
				.filter(|u| {
					u.input_tokens <= i64::MAX as u64 && u.output_tokens <= i64::MAX as u64
				});

			if let Some(usage) = usage {
				usage.details = Some(usage_details(
					saved
						.iter()
						.find(|saved| &saved.turn_id == turn_id)
						.and_then(|saved| saved.observation_json.as_deref()),
					responses
						.iter()
						.find(|row| &row.turn_id == turn_id)
						.map(|row| row.observed_count as u64),
				));
			}

			let tokens = saved.iter().find(|saved| &saved.turn_id == turn_id).and_then(summary);
			let response = response_summary(&responses, turn_id);
			let parts = [tokens, response].into_iter().flatten().collect::<Vec<_>>();

			*usage_summary = (!parts.is_empty()).then(|| parts.join("\n"));
		}
	}

	Ok(())
}

fn usage_details(
	observation: Option<&str>,
	responses: Option<u64>,
) -> decodex_protocol::AgentUsageDetailsDto {
	let mut details = decodex_protocol::AgentUsageDetailsDto { responses, ..Default::default() };

	if let Some(usage) = observation
		.and_then(|json| serde_json::from_str::<ThreadTokenUsage>(json).ok())
		.filter(|usage| usage.is_valid())
	{
		details.last_input = Some(usage.last.input_tokens);
		details.cached_input = Some(usage.last.cached_input_tokens);
		details.last_output = Some(usage.last.output_tokens);
		details.reasoning_output = Some(usage.last.reasoning_output_tokens);
		details.thread_total = Some(usage.total.total_tokens);
		details.context_capacity = usage.model_context_window;
	}

	details
}

fn response_summary(rows: &[AgentResponseUsageSummary], turn: &str) -> Option<String> {
	let rows = rows.iter().filter(|row| row.turn_id == turn).collect::<Vec<_>>();
	let first = rows.first()?;
	let mut lines = vec![format!(
		"Observed responses: {}. Showing {} recorded amounts; units are provider-defined.",
		first.observed_count,
		rows.len()
	)];

	for row in rows {
		let amount = row
			.amount
			.as_deref()
			.filter(|amount| {
				!amount.is_empty()
					&& amount.len() <= 256
					&& amount.bytes().all(|byte| {
						byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+' | b'e' | b'E')
					})
			})
			.unwrap_or("unknown");
		let identity = super::visible_text(&row.response_id).0;

		lines.push(format!(
			"Response {identity:?}: {amount}.{}",
			if row.metadata_omitted { " Extended metadata omitted due to size." } else { "" }
		));
	}

	Some(lines.join("\n"))
}

fn summary(saved: &AgentTurnMetrics) -> Option<String> {
	let mut parts = Vec::new();

	if let Some(usage) = saved
		.usage_json
		.as_deref()
		.and_then(|value| serde_json::from_str::<AgentTurnUsageDto>(value).ok())
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
		.and_then(|value| observations::usage_text(&value))
	{
		parts.push(observation);
	}

	(!parts.is_empty()).then(|| parts.join("\n"))
}

#[cfg(test)]
mod tests {
	use std::{
		future,
		sync::atomic::{AtomicUsize, Ordering},
	};

	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use crate::{
		agent::timeline::metrics::{self, AgentTimelineContent},
		agent_usage_estimate::{Source, SourceKey},
	};
	use decodex_codex::app_server_client::AppServerClient;
	use decodex_core::{AccountId, DecodexRoot, ProcessGenerationId};
	use decodex_database::{
		AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus, EnqueueAgentEvent,
		SqliteStore,
	};

	#[test]
	fn structured_details_keep_response_totals_separate_and_missing_values_unknown() {
		let missing = metrics::usage_details(None, Some(2));

		assert_eq!(missing.responses, Some(2));
		assert_eq!(missing.last_input, None);

		let counts = serde_json::json!({"totalTokens":63_197,"inputTokens":63_045,"cachedInputTokens":62_592,"outputTokens":152,"reasoningOutputTokens":85});
		let value = serde_json::json!({"last":counts,"total":{"totalTokens":2_759_729,"inputTokens":2_700_000,"cachedInputTokens":2_000_000,"outputTokens":59_729,"reasoningOutputTokens":0},"modelContextWindow":380_000}).to_string();
		let details = metrics::usage_details(Some(&value), Some(2));

		assert_eq!(details.last_input, Some(63_045));
		assert_eq!(details.cached_input, Some(62_592));
		assert_eq!(details.last_output, Some(152));
		assert_eq!(details.reasoning_output, Some(85));
		assert_eq!(details.thread_total, Some(2_759_729));
		assert_eq!(details.context_capacity, Some(380_000));
		assert_eq!(metrics::usage_details(Some("{}"), None).last_output, None);
	}

	#[test]
	fn response_amounts_preserve_precision_and_missing_values_without_currency_assumptions() {
		let rows = vec![
			decodex_database::AgentResponseUsageSummary {
				turn_id: "turn".into(),
				response_id: "a".into(),
				amount: Some("0.12345678901234567890".into()),
				metadata_omitted: false,
				observed_count: 3,
			},
			decodex_database::AgentResponseUsageSummary {
				turn_id: "turn".into(),
				response_id: "b".into(),
				amount: Some("0".into()),
				metadata_omitted: false,
				observed_count: 3,
			},
			decodex_database::AgentResponseUsageSummary {
				turn_id: "turn".into(),
				response_id: "c".into(),
				amount: None,
				metadata_omitted: true,
				observed_count: 3,
			},
		];
		let text = metrics::response_summary(&rows, "turn").unwrap();

		assert!(text.contains("0.12345678901234567890"));
		assert!(text.contains("Response \"b\": 0."));
		assert!(text.contains("Response \"c\": unknown."));
		assert!(text.contains("Extended metadata omitted due to size."));
		assert!(!text.contains('$'));
		assert!(metrics::response_summary(&rows, "other-turn").is_none());
	}
	#[test]
	fn whole_turn_delta_and_last_response_observation_keep_distinct_labels() {
		let counts = serde_json::json!({"totalTokens":150,"inputTokens":120,"cachedInputTokens":50,"cacheWriteInputTokens":10,"outputTokens":30,"reasoningOutputTokens":8});
		let mut saved = decodex_database::AgentTurnMetrics {
			turn_id: "turn".into(),
			usage_json: Some(serde_json::json!({"input_tokens":240,"output_tokens":60}).to_string()),
			observation_json: Some(
				serde_json::json!({"last":counts,"total":{"totalTokens":900,"inputTokens":700,"cachedInputTokens":200,"cacheWriteInputTokens":20,"outputTokens":200,"reasoningOutputTokens":100},"modelContextWindow":128_000}).to_string(),
			),
		};
		let text = metrics::summary(&saved).unwrap();

		assert!(text.contains("Turn tokens: input 240, output 60."));
		assert!(text.contains(
			"Last response tokens: input 120, cached input 50, output 30, reasoning output 8."
		));
		assert!(text.contains("Cache write input tokens: 10."));
		assert!(text.contains("Thread total tokens: 900."));

		saved.usage_json = None;

		assert!(!metrics::summary(&saved).unwrap().contains("Turn tokens:"));

		saved.observation_json = None;

		assert_eq!(metrics::summary(&saved), None);

		saved.usage_json =
			Some(serde_json::json!({"input_tokens":0,"output_tokens":0}).to_string());

		assert_eq!(metrics::summary(&saved).as_deref(), Some("Turn tokens: input 0, output 0."));

		saved.usage_json =
			Some(serde_json::json!({"input_tokens":u64::MAX,"output_tokens":0}).to_string());

		assert_eq!(metrics::summary(&saved), None);
	}
	#[tokio::test]
	async fn native_page_reads_enrich_exact_turns_and_recheck_source_after_saved_usage() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();

		store
			.create_agent_work_item(AgentWorkItem {
				id: "work".into(),
				parent_goal_id: None,
				kind: AgentWorkKind::Goal,
				title: "Work".into(),
				instructions: "Task".into(),
				codex_thread_id: None,
				dispatch_state: AgentDispatchState::Idle,
				active_turn_id: None,
				status: AgentWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:serde_json::json!(["turn/completed","thread","turn"]).to_string(),work_item_id:"work".into(),event_kind:"agent_turn_completed".into(),payload:serde_json::json!({"terminal":{"threadId":"thread","turn":{"id":"turn"}},"usage":{"input_tokens":11,"output_tokens":2}}).to_string()}).await.unwrap();

		for change in [false, true] {
			let (local, remote) = io::duplex(8_192);
			let (reader, writer) = io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = io::split(remote);
				let mut lines = BufReader::new(reader).lines();

				for (method, result) in [
					("thread/read", serde_json::json!({"thread":{"id":"thread"}})),
					(
						"thread/timeline/list",
						serde_json::json!({"data":[{"type":"turnCompleted","position":1,"turnId":"turn","status":"completed","error":null},{"type":"turnCompleted","position":2,"turnId":"unreported","status":"completed","error":null}],"nextCursor":null,"activeRealtimeSessionAtPageStart":null}),
					),
				] {
					let request: serde_json::Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

					assert_eq!(request["method"], method);

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
			let result = super::super::read(
				Some(&store),
				|| {
					let after = calls.fetch_add(1, Ordering::SeqCst) >= 3;

					future::ready(Some(Source {
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
				assert_eq!(result, decodex_protocol::AgentTimelineResult::Unavailable);
			} else {
				let decodex_protocol::AgentTimelineResult::Available { page, .. } = result else {
					panic!("available native page")
				};

				assert!(
					matches!(&page.entries[0].content,AgentTimelineContent::TurnBoundary {usage_summary:Some(text),..} if text=="Turn tokens: input 11, output 2.")
				);
				assert!(matches!(
					&page.entries[1].content,
					AgentTimelineContent::TurnBoundary { usage_summary: None, .. }
				));
			}
		}
	}
}
