//! Native turn headers own elapsed time. Transport observations exclude tools from speed.
use decodex_codex::{
	ThreadTokenUsage,
	app_server_client::{AppServerClient, ObservedOutputRate},
};
use decodex_database::SqliteStore;
use decodex_protocol::{AgentOutputRate, AgentThreadMetrics};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) async fn read(
	client: &AppServerClient,
	store: &SqliteStore,
	work: &str,
	thread: &str,
) -> Option<AgentThreadMetrics> {
	// This existing native path pages only turn headers; never load transcript bodies to count
	// time.
	let turns = client.thread_turns_since(thread, None).await.ok()?;
	let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()?;
	let mut result = durations(&turns, now);
	result.generating = client.is_generating_output(thread);
	// Keep the last generation measurement while another response is pending. Exact native
	// counters and transport time are independent of UI playback and tool completion.
	if let Some(event) =
		store.read_agent_output_rate_observation(work.into(), thread.into()).await.ok().flatten()
		&& let Ok(value) = serde_json::from_str::<Value>(&event.payload)
		&& let Some(turn) = turns.iter().find(|turn| turn["id"] == value["turnId"])
		&& let Ok(rate) =
			serde_json::from_value::<ObservedOutputRate>(value["observedOutputRate"].clone())
		&& rate.duration_ms > 0
	{
		result.rate = Some(AgentOutputRate {
			estimated: true,
			active: turn["status"] == "inProgress" && result.generating,
			output_tokens: rate.output_tokens,
			duration_ms: rate.duration_ms,
			started_at_ms: rate.started_at_ms,
			completed_at_ms: Some(rate.completed_at_ms),
		});
		return Some(result);
	}
	// A whole-turn average is historical; a running turn can be waiting on tools or a user.
	for index in (turns.len().saturating_sub(2)..turns.len()).rev() {
		let turn = &turns[index];
		if turn["status"] == "inProgress" {
			continue;
		}
		let current = output(store, work, thread, turn["id"].as_str()?).await;
		let before = if index == 0 {
			Some(0)
		} else {
			output(store, work, thread, turns[index - 1]["id"].as_str()?).await
		};
		let Some(output_tokens) = current.zip(before).and_then(|(a, b)| a.checked_sub(b)) else {
			continue;
		};
		let Some(started_at_ms) =
			turn["startedAt"].as_u64().and_then(|value| value.checked_mul(1000))
		else {
			continue;
		};
		let duration_ms = turn["durationMs"].as_u64();
		if let Some(duration_ms) = duration_ms.filter(|ms| *ms > 0) {
			result.rate = Some(AgentOutputRate {
				estimated: false,
				active: false,
				output_tokens,
				duration_ms,
				started_at_ms,
				completed_at_ms: Some(started_at_ms.saturating_add(duration_ms)),
			});
			break;
		}
	}
	Some(result)
}

fn durations(turns: &[Value], now: u64) -> AgentThreadMetrics {
	let completed_ms = turns
		.iter()
		.filter(|turn| turn["status"] != "inProgress")
		.try_fold(0u64, |sum, turn| sum.checked_add(turn["durationMs"].as_u64()?));
	let active_ms = turns
		.iter()
		.rev()
		.find(|turn| turn["status"] == "inProgress")
		.and_then(|turn| now.checked_sub(turn["startedAt"].as_u64()?.checked_mul(1000)?));
	AgentThreadMetrics { completed_ms, active_ms, ..Default::default() }
}

async fn output(store: &SqliteStore, work: &str, thread: &str, turn: &str) -> Option<u64> {
	let value = observation(store, work, thread, turn).await?;
	let usage: ThreadTokenUsage = serde_json::from_value(value["tokenUsage"].clone()).ok()?;
	usage.is_valid().then_some(usage.total.output_tokens)
}

async fn observation(store: &SqliteStore, work: &str, thread: &str, turn: &str) -> Option<Value> {
	let event = store
		.read_agent_usage_observation(work.into(), thread.into(), turn.into())
		.await
		.ok()??;
	serde_json::from_str(&event.payload).ok()
}

#[cfg(test)]
mod tests {
	use super::*;
	#[tokio::test]
	async fn child_statistics_use_all_native_turns_and_exact_output_deltas_after_reopen() {
		use crate::agent::tests;
		use decodex_codex::app_server_client::ServerEvent;
		let history = serde_json::json!({"child":{"thread":{"id":"child","parentThreadId":"opaque thread/1","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"opaque thread/1"}}},"historyMode":"paginated","turns":[
			{"id":"second","status":"completed","startedAt":200,"durationMs":3000,"items":[]},
			{"id":"first","status":"completed","startedAt":100,"durationMs":1000,"items":[]}
		]}}});
		let (mut agent, _sent, _directory) = tests::fixture_with_history(history).await;
		agent.start_agent("agent", "Coordinate").await.unwrap();
		for (turn, output) in [("first", 100), ("second", 160)] {
			let counts = serde_json::json!({"totalTokens":1000+output,"inputTokens":1000,"cachedInputTokens":0,"outputTokens":output,"reasoningOutputTokens":0});
			agent.handle_event(ServerEvent::Notification { method: "thread/tokenUsage/updated".into(), params: serde_json::json!({"threadId":"child","turnId":turn,"tokenUsage":{"total":counts,"last":counts,"modelContextWindow":128000}}) }).await.unwrap();
		}
		for _ in 0..2 {
			let stats = read(&agent.client, &agent.store, "agent", "child").await.unwrap();
			assert_eq!(stats.completed_ms, Some(4000));
			assert_eq!(stats.active_ms, None);
			let rate = stats.rate.unwrap();
			assert_eq!(
				rate.output_tokens, 60,
				"use the output delta, never prompt tokens or cumulative output"
			);
			assert_eq!(rate.duration_ms, 3000);
			assert_eq!(rate.completed_at_ms, Some(203_000));
			assert!(!rate.estimated && !rate.active);
			agent = crate::agent::AgentCoordinator::new(
				agent.store.clone(),
				agent.client.clone(),
				agent.config.clone(),
			)
			.unwrap();
		}
		for (source, measured) in [("measured", true), ("later-unmeasured", false)] {
			let mut value = serde_json::json!({"threadId":"child","turnId":"second"});
			if measured {
				value["observedOutputRate"] = serde_json::to_value(ObservedOutputRate {
					output_tokens: 60,
					duration_ms: 500,
					started_at_ms: 201000,
					completed_at_ms: 201500,
				})
				.unwrap();
			}
			agent
				.store
				.record_agent_observation(decodex_database::EnqueueAgentEvent {
					source_event_id: source.into(),
					work_item_id: "agent".into(),
					event_kind: "token_usage".into(),
					payload: value.to_string(),
				})
				.await
				.unwrap();
		}
		agent = crate::agent::AgentCoordinator::new(
			agent.store.clone(),
			agent.client.clone(),
			agent.config.clone(),
		)
		.unwrap();
		let measured =
			read(&agent.client, &agent.store, "agent", "child").await.unwrap().rate.unwrap();
		assert!(measured.estimated && !measured.active);
		assert_eq!(
			(measured.output_tokens, measured.duration_ms),
			(60, 500),
			"later unmeasured responses and reopening retain Last, never the tool wait"
		);
		assert!(agent.store.list_pending_agent_events(10).await.unwrap().is_empty());
	}

	#[test]
	fn complete_history_time_does_not_depend_on_the_visible_page_or_count_idle_time() {
		let turns = vec![
			serde_json::json!({"id":"one","status":"completed","startedAt":1,"durationMs":1250}),
			serde_json::json!({"id":"two","status":"failed","startedAt":90,"durationMs":750}),
			serde_json::json!({"id":"live","status":"inProgress","startedAt":200}),
		];
		let stats = durations(&turns, 203_000);
		assert_eq!(stats.completed_ms, Some(2000));
		assert_eq!(stats.active_ms, Some(3000));
		let mut missing = turns;
		missing[0]["durationMs"] = Value::Null;
		assert_eq!(durations(&missing, 203_000).completed_ms, None);
	}
}
