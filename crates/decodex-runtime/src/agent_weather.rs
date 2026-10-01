//! Narrow compatibility adapter until app-server exposes web weather results.
//! Read only the path returned for the selected native thread, never scan sessions.
use std::{
	collections::{BTreeSet, HashMap},
	fs::File,
	io::{BufRead as _, BufReader, Read as _},
	time::Duration,
};

use serde_json::Value;
use tokio::{task, time};

use crate::agent_host::AgentHost;
use decodex_protocol::{
	AgentHistoryEntryDto, AgentHistoryResult, AgentTimelinePage, WeatherForecast,
};

pub(super) struct CachedWeather {
	thread: String,
	turns: HashMap<String, Vec<WeatherForecast>>,
}

impl AgentHost {
	async fn weather_for_turns(
		&self,
		thread: &str,
		turns: &BTreeSet<String>,
	) -> HashMap<String, Vec<WeatherForecast>> {
		if turns.is_empty() {
			return HashMap::new();
		}

		if let Some(cache) = self.weather_cache.lock().await.as_ref()
			&& cache.thread == thread
			&& turns.iter().all(|turn| cache.turns.contains_key(turn))
		{
			return cache
				.turns
				.iter()
				.filter(|(turn, _)| turns.contains(*turn))
				.map(|(k, v)| (k.clone(), v.clone()))
				.collect();
		}

		let Some((_, client)) = self.runtime.agent_catalog_client() else {
			return HashMap::new();
		};
		let Ok(Ok(readback)) = time::timeout(
			Duration::from_secs(2),
			client.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false})),
		)
		.await
		else {
			return HashMap::new();
		};

		if readback.pointer("/thread/id").and_then(Value::as_str) != Some(thread) {
			return HashMap::new();
		}

		let Some(path) =
			readback.pointer("/thread/path").and_then(Value::as_str).map(str::to_owned)
		else {
			return HashMap::new();
		};
		let source_thread = thread.to_owned();
		let Ok(weather) = task::spawn_blocking(move || read_weather(&path, &source_thread)).await
		else {
			return HashMap::new();
		};
		let selected = weather
			.iter()
			.filter(|(turn, _)| turns.contains(*turn))
			.map(|(k, v)| (k.clone(), v.clone()))
			.collect();

		*self.weather_cache.lock().await =
			Some(CachedWeather { thread: thread.into(), turns: weather });

		selected
	}

	pub(crate) async fn enrich_timeline_weather(&self, page: &mut AgentTimelinePage) {
		use decodex_protocol::AgentTimelineContent as Content;

		let turns = page
			.entries
			.iter()
			.filter_map(|entry| match &entry.content {
				Content::Item { kind, text, turn_id, .. }
					if kind == "agentMessage" && has_weather(text) =>
					Some(turn_id.clone()),
				_ => None,
			})
			.collect();

		page.weather = self.weather_for_turns(&page.thread_id, &turns).await.into_iter().collect();
		// Preserve the native page's existing wire budget; prose remains readable if a card cannot
		// fit.
		while !page.weather.is_empty()
			&& serde_json::to_vec(page).map_or(true, |bytes| bytes.len() > 60 * 1_024)
		{
			page.weather.pop_last();
		}
	}

	pub(crate) async fn enrich_weather(&self, work_id: &str, history: &mut AgentHistoryResult) {
		let AgentHistoryResult::Available { entries, .. } = history else {
			return;
		};
		let turns: BTreeSet<_> = entries
			.iter()
			.filter(|entry| entry.kind == "assistant" && has_weather(&entry.text))
			.filter_map(|entry| entry.turn_id.clone())
			.collect();

		if turns.is_empty() {
			return;
		}

		let Ok(work) = self.store.get_agent_work_item(work_id.into()).await else {
			return;
		};
		let Some(thread) = work.codex_thread_id else {
			return;
		};

		attach(entries, &self.weather_for_turns(&thread, &turns).await);
	}
}

fn has_weather(text: &str) -> bool {
	["weather", "forecast"].iter().any(|kind| text.contains(&format!("\u{e200}{kind}\u{e202}")))
}

fn attach(entries: &mut [AgentHistoryEntryDto], weather: &HashMap<String, Vec<WeatherForecast>>) {
	for entry in entries {
		if entry.kind != "assistant" {
			continue;
		}

		let Some(turn) = &entry.turn_id else { continue };

		if let Some(forecasts) = weather.get(turn) {
			entry.weather = forecasts
				.iter()
				.filter(|forecast| {
					["weather", "forecast"].iter().any(|kind| {
						entry.text.contains(&format!(
							"\u{e200}{kind}\u{e202}{}\u{e201}",
							forecast.reference
						))
					})
				})
				.take(4)
				.cloned()
				.collect();
		}
	}
}

fn read_weather(path: &str, thread: &str) -> HashMap<String, Vec<WeatherForecast>> {
	let mut result = HashMap::new();
	let Ok(file) = File::open(path) else { return result };

	if !file.metadata().is_ok_and(|m| m.is_file() && m.len() <= 16 * 1_024 * 1_024) {
		return result;
	}

	let mut lines = BufReader::new(file.take(16 * 1_024 * 1_024)).lines();
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
			let Some(text) = item["text"].as_str().filter(|text| text.len() <= 8_192) else {
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
	use std::{io::Write as _, slice};

	use crate::agent_host::weather::{self, HashMap, WeatherForecast};

	#[test]
	fn weather_and_forecast_aliases_attach_only_exact_assistant_references() {
		let forecast = WeatherForecast::parse(include_str!(
			"../../../apps/decodex-gpui/examples/fixtures/singapore-weather.txt"
		))
		.unwrap();
		let weather = HashMap::from([("turn-a".into(), vec![forecast.clone()])]);
		let mut entry = decodex_protocol::AgentHistoryEntryDto {
			native_source: None,
			id: 1,
			kind: "assistant".into(),
			text: String::new(),
			created_at_micros: 0,
			duration_ms: None,
			usage: None,
			activity: None,
			receipt: None,
			turn_id: Some("turn-a".into()),
			weather: vec![],
		};

		for marker in ["weather", "forecast"] {
			entry.text = format!("\u{e200}{marker}\u{e202}{}\u{e201}", forecast.reference);

			weather::attach(slice::from_mut(&mut entry), &weather);

			assert_eq!(entry.weather, vec![forecast.clone()]);
		}

		entry.text = "\u{e200}forecast\u{e202}unrelated\u{e201}".into();

		weather::attach(slice::from_mut(&mut entry), &weather);

		assert!(entry.weather.is_empty());
	}

	#[test]
	fn weather_history_is_bound_to_the_exact_thread_and_turn() {
		let mut file = tempfile::NamedTempFile::new().unwrap();
		let fixture =
			include_str!("../../../apps/decodex-gpui/examples/fixtures/singapore-weather.txt");

		for record in [
			serde_json::json!({"type":"session_meta","payload":{"id":"thread-a"}}),
			serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-a"}}),
			serde_json::json!({"type":"response_item","payload":{"type":"custom_tool_call_output","output":[{"type":"input_text","text":fixture}]}}),
		] {
			writeln!(file, "{record}").unwrap();
		}

		let path = file.path().to_str().unwrap();

		assert!(weather::read_weather(path, "another-thread").is_empty());

		let results = weather::read_weather(path, "thread-a");

		assert_eq!(results["turn-a"][0].celsius, 32);
		assert_eq!(results["turn-a"][0].hours.len(), 12);
		assert!(!results.contains_key("another-turn"));
	}
}
