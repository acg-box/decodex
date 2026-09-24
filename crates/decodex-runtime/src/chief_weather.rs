//! Narrow compatibility adapter until app-server exposes web weather results.
//! Read only the path returned for the selected native thread, never scan sessions.
use crate::chief_host::ChiefHost;
use decodex_protocol::{ChiefHistoryResult, WeatherForecast};
use serde_json::{Value, json};
use std::{
	collections::HashMap,
	io::{BufRead, BufReader, Read},
};

pub(super) struct CachedWeather {
	thread: String,
	turns: HashMap<String, Vec<WeatherForecast>>,
}
fn attach(
	entries: &mut [decodex_protocol::ChiefHistoryEntryDto],
	weather: &HashMap<String, Vec<WeatherForecast>>,
) {
	for entry in entries {
		let Some(turn) = &entry.turn_id else { continue };
		if let Some(forecasts) = weather.get(turn) {
			entry.weather = forecasts
				.iter()
				.filter(|forecast| {
					entry
						.text
						.contains(&format!("\u{e200}weather\u{e202}{}\u{e201}", forecast.reference))
				})
				.take(4)
				.cloned()
				.collect();
		}
	}
}

impl ChiefHost {
	pub(crate) async fn enrich_weather(&self, work_id: &str, history: &mut ChiefHistoryResult) {
		let ChiefHistoryResult::Available { entries, .. } = history else { return };
		if !entries.iter().any(|entry| {
			entry.kind == "assistant" && entry.text.contains("\u{e200}weather\u{e202}")
		}) {
			return;
		}
		let Ok(work) = self.store.get_chief_work_item(work_id.into()).await else { return };
		let Some(thread) = work.codex_thread_id else { return };
		if let Some(cache) = self.weather_cache.lock().await.as_ref()
			&& cache.thread == thread
			&& entries
				.iter()
				.filter(|e| e.text.contains("\u{e200}weather\u{e202}"))
				.all(|e| e.turn_id.as_ref().is_some_and(|turn| cache.turns.contains_key(turn)))
		{
			attach(entries, &cache.turns);
			return;
		}

		let Some((_, client)) = self.runtime.chief_catalog_client() else { return };
		let Ok(Ok(readback)) = tokio::time::timeout(
			std::time::Duration::from_secs(2),
			client.thread_read(json!({"threadId":thread,"includeTurns":false})),
		)
		.await
		else {
			return;
		};
		if readback.pointer("/thread/id").and_then(Value::as_str) != Some(thread.as_str()) {
			return;
		}
		let Some(path) =
			readback.pointer("/thread/path").and_then(Value::as_str).map(str::to_owned)
		else {
			return;
		};
		let source_thread = thread.clone();
		let Ok(weather) =
			tokio::task::spawn_blocking(move || read_weather(&path, &source_thread)).await
		else {
			return;
		};
		attach(entries, &weather);
		*self.weather_cache.lock().await = Some(CachedWeather { thread, turns: weather });
	}
}
fn read_weather(path: &str, thread: &str) -> HashMap<String, Vec<WeatherForecast>> {
	let mut result = HashMap::new();
	let Ok(file) = std::fs::File::open(path) else { return result };
	if !file.metadata().is_ok_and(|m| m.is_file() && m.len() <= 16 * 1024 * 1024) {
		return result;
	}
	let mut lines = BufReader::new(file.take(16 * 1024 * 1024)).lines();
	let Some(Ok(first)) = lines.next() else { return result };
	let Ok(header) = serde_json::from_str::<Value>(&first) else { return result };
	if header["type"] != "session_meta" || header["payload"]["id"] != thread {
		return result;
	}
	let mut turn = String::new();
	for line in lines.map_while(Result::ok) {
		let Ok(record) = serde_json::from_str::<Value>(&line) else { continue };
		let payload = &record["payload"];
		if record["type"] == "event_msg" && payload["type"] == "task_started" {
			turn = payload["turn_id"].as_str().unwrap_or_default().into();
		}
		if turn.is_empty()
			|| record["type"] != "response_item"
			|| payload["type"] != "custom_tool_call_output"
		{
			continue;
		}
		for item in payload["output"].as_array().into_iter().flatten() {
			let Some(text) = item["text"].as_str().filter(|text| text.len() <= 8192) else {
				continue;
			};
			if let Some(forecast) = WeatherForecast::parse(text) {
				let forecasts: &mut Vec<WeatherForecast> = result.entry(turn.clone()).or_default();
				if forecasts.len() < 4
					&& !forecasts.iter().any(|f| f.reference == forecast.reference)
				{
					forecasts.push(forecast);
				}
			}
		}
	}
	result
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn weather_history_is_bound_to_the_exact_thread_and_turn() {
		use std::io::Write;
		let mut file = tempfile::NamedTempFile::new().unwrap();
		let fixture =
			include_str!("../../../apps/decodex-gpui/examples/fixtures/singapore-weather.txt");
		for record in [
			json!({"type":"session_meta","payload":{"id":"thread-a"}}),
			json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-a"}}),
			json!({"type":"response_item","payload":{"type":"custom_tool_call_output","output":[{"type":"input_text","text":fixture}]}}),
		] {
			writeln!(file, "{record}").unwrap();
		}
		let path = file.path().to_str().unwrap();
		assert!(read_weather(path, "another-thread").is_empty());
		let results = read_weather(path, "thread-a");
		assert_eq!(results["turn-a"][0].celsius, 32);
		assert_eq!(results["turn-a"][0].hours.len(), 12);
		assert!(!results.contains_key("another-turn"));
	}
}
