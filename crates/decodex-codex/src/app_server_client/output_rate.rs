//! Measure native generation before the owner's queue, tool execution, and UI playback.
use std::{
	collections::{HashMap, VecDeque},
	sync::{Arc, Mutex},
	time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
	ThreadTokenUsage, TokenUsageBreakdown,
	app_server_client::{AppServerClient, ServerEvent},
};

/// An observed generation interval, paired with exact native response output counters.
/// This is an estimate of network-observed throughput, not provider GPU generation time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ObservedOutputRate {
	/// Native output tokens. Reasoning is included only when its start was observed.
	pub output_tokens: u64,
	/// Monotonic elapsed time from the first model item to the end of generation.
	pub duration_ms: u64,
	/// Local wall time used only to group overlapping agents, never to measure duration.
	pub started_at_ms: u64,
	/// Local end time derived from the monotonic interval.
	pub completed_at_ms: u64,
}

struct Window {
	turn: String,
	start: Instant,
	start_ms: u64,
	end: Option<Instant>,
	reasoning: bool,
	sealed: bool,
	tool: bool,
	generating: bool,
	raw_usage: Option<TokenUsageBreakdown>,
}
impl Window {
	fn finish(&self, usage: &TokenUsageBreakdown) -> Option<ObservedOutputRate> {
		// Cold resumes can omit rawResponse/completed. Tool starts can be delayed by
		// approvals or other tools, so require that boundary for tool-bearing responses.
		if (self.tool && !self.sealed) || self.raw_usage.as_ref().is_some_and(|raw| raw != usage) {
			return None;
		}
		let duration_ms = u64::try_from(self.end?.duration_since(self.start).as_millis()).ok()?;
		// Batched/replayed markers are not a useful speed measurement.
		if duration_ms < 100 {
			return None;
		}
		let output_tokens = if self.reasoning {
			usage.output_tokens
		} else {
			usage.output_tokens.checked_sub(usage.reasoning_output_tokens)?
		};
		(output_tokens > 0).then_some(ObservedOutputRate {
			output_tokens,
			duration_ms,
			started_at_ms: self.start_ms,
			completed_at_ms: self.start_ms.saturating_add(duration_ms),
		})
	}
}

struct Receipt {
	thread: String,
	turn: String,
	usage: ThreadTokenUsage,
	rate: Option<ObservedOutputRate>,
}
#[derive(Default)]
struct State {
	windows: HashMap<String, Window>,
	receipts: VecDeque<Receipt>,
}

#[derive(Clone, Default)]
pub(super) struct OutputRates(Arc<Mutex<State>>);
impl OutputRates {
	pub(super) fn generating(&self, thread: &str) -> bool {
		self.0.lock().is_ok_and(|state| state.windows.get(thread).is_some_and(|w| w.generating))
	}

	pub(super) fn clear(&self) {
		if let Ok(mut state) = self.0.lock() {
			*state = State::default();
		}
	}

	pub(super) fn take(
		&self,
		thread: &str,
		turn: &str,
		usage: &ThreadTokenUsage,
	) -> Option<ObservedOutputRate> {
		let mut state = self.0.lock().ok()?;
		let index = state
			.receipts
			.iter()
			.position(|r| r.thread == thread && r.turn == turn && &r.usage == usage)?;
		state.receipts.remove(index)?.rate
	}

	pub(super) fn observe(&self, event: &ServerEvent) {
		if let ServerEvent::Request { params, .. } = event {
			if let (Some(thread), Some(turn), Ok(mut state)) =
				(params["threadId"].as_str(), params["turnId"].as_str(), self.0.lock())
				&& let Some(window) = state.windows.get_mut(thread).filter(|w| w.turn == turn)
			{
				window.tool = true;
				window.generating = false;
			}
			return;
		}
		let ServerEvent::Notification { method, .. } = event else { return };
		if !matches!(
			method.as_str(),
			"item/started"
				| "item/completed"
				| "rawResponse/completed"
				| "thread/tokenUsage/updated"
				| "turn/started"
				| "turn/completed"
				| "thread/closed"
				| "thread/archived"
				| "thread/deleted"
				| "thread/reverted"
		) {
			return;
		}
		let wall = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.ok()
			.and_then(|d| u64::try_from(d.as_millis()).ok());
		if let Some(wall) = wall {
			self.observe_at(event, Instant::now(), wall);
		}
	}

	fn observe_at(&self, event: &ServerEvent, now: Instant, wall: u64) {
		let ServerEvent::Notification { method, params } = event else { return };
		let Some(thread) = params["threadId"].as_str() else { return };
		let Ok(mut state) = self.0.lock() else { return };
		if matches!(
			method.as_str(),
			"thread/closed"
				| "thread/archived"
				| "thread/deleted"
				| "thread/reverted"
				| "turn/started"
				| "turn/completed"
		) {
			if method != "turn/completed"
				|| state
					.windows
					.get(thread)
					.is_some_and(|w| params["turn"]["id"].as_str() == Some(&w.turn))
			{
				state.windows.remove(thread);
			}
			return;
		}
		let Some(turn) = params["turnId"].as_str() else { return };
		if method == "thread/tokenUsage/updated" {
			let Ok(usage) =
				serde_json::from_value::<ThreadTokenUsage>(params["tokenUsage"].clone())
			else {
				return;
			};
			if !usage.is_valid() {
				return;
			}
			let rate = if state.windows.get(thread).is_some_and(|w| w.turn == turn) {
				state.windows.remove(thread).and_then(|w| w.finish(&usage.last))
			} else {
				None
			};
			// At most one receipt per queued native event. Exact counters prevent a slow owner
			// from associating an earlier response with a later measurement on the same turn.
			if state.receipts.len() == super::MAX_BUFFERED_EVENTS {
				state.receipts.pop_front();
			}
			state.receipts.push_back(Receipt {
				thread: thread.into(),
				turn: turn.into(),
				usage,
				rate,
			});
			return;
		}
		let model =
			matches!(params["item"]["type"].as_str(), Some("reasoning" | "agentMessage" | "plan"))
				&& params["item"]["delivery"] != "async";
		if method == "item/started" && model {
			if state.windows.get(thread).is_none_or(|w| w.turn != turn) {
				state.windows.insert(
					thread.into(),
					Window {
						turn: turn.into(),
						start: now,
						start_ms: wall,
						end: None,
						reasoning: false,
						sealed: false,
						tool: false,
						generating: true,
						raw_usage: None,
					},
				);
			}
			if let Some(window) = state.windows.get_mut(thread).filter(|w| !w.sealed) {
				window.reasoning |= params["item"]["type"] == "reasoning";
				window.generating = true;
			}
		} else if let Some(window) =
			state.windows.get_mut(thread).filter(|w| w.turn == turn && !w.sealed)
		{
			if method == "item/completed" && model {
				window.end = Some(now);
				window.generating = false;
			}
			// Tool lifecycles can include approvals and waits; they do not time generation.
			let tool = matches!(
				params["item"]["type"].as_str(),
				Some(
					"commandExecution"
						| "dynamicToolCall"
						| "mcpToolCall"
						| "collabAgentToolCall"
						| "webSearch"
						| "fileChange"
						| "imageGeneration"
						| "imageView"
						| "sleep"
				)
			);
			window.tool |= method == "item/started" && tool;
			if method == "item/started" && tool {
				window.generating = false;
			}
			if method == "rawResponse/completed" {
				window.end = Some(now);
				window.sealed = true;
				window.generating = false;
				window.raw_usage =
					serde_json::from_value::<TokenUsageBreakdown>(params["usage"].clone())
						.ok()
						.filter(TokenUsageBreakdown::is_valid);
			}
		}
	}
}

impl AppServerClient {
	/// Whether a native model item is currently streaming, rather than waiting on a tool or user.
	pub fn is_generating_output(&self, thread: &str) -> bool {
		!(*self.closed.borrow() || self.outbound.is_closed())
			&& self.server_requests.generating_output(thread)
	}

	/// Consume the measurement for this exact queued usage event. No owner queue time is added.
	pub fn take_observed_output_rate(
		&self,
		thread: &str,
		turn: &str,
		usage: &ThreadTokenUsage,
	) -> Option<ObservedOutputRate> {
		self.server_requests.take_output_rate(thread, turn, usage)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	use std::time::Duration;
	fn counts(output: u64, reasoning: u64) -> serde_json::Value {
		json!({"totalTokens":1000+output,"inputTokens":1000,"cachedInputTokens":0,"outputTokens":output,"reasoningOutputTokens":reasoning})
	}
	#[test]
	fn delayed_usage_excludes_tools_and_questions_and_pairs_queued_responses_exactly() {
		let rates = OutputRates::default();
		let clock = Instant::now();
		let emit = |ms, method: &str, item| {
			rates.observe_at(
				&ServerEvent::Notification {
					method: method.into(),
					params: json!({"threadId":"child","turnId":"turn","item":item}),
				},
				clock + Duration::from_millis(ms),
				100_000 + ms,
			)
		};
		emit(0, "item/started", json!({"type":"reasoning"}));
		assert!(rates.generating("child"));
		emit(1800, "item/completed", json!({"type":"reasoning"}));
		assert!(!rates.generating("child"));
		emit(2000, "item/started", json!({"type":"dynamicToolCall"}));
		emit(3000, "item/started", json!({"type":"commandExecution"}));
		emit(3500, "rawResponse/completed", serde_json::Value::Null);
		emit(120000, "item/completed", json!({"type":"dynamicToolCall"}));
		assert!(!rates.generating("child"), "a running turn can be waiting without generating");
		let usage = |total, last, reasoning| {
			serde_json::from_value::<ThreadTokenUsage>(json!({"total":counts(total, reasoning),"last":counts(last, reasoning),"modelContextWindow":null})).unwrap()
		};
		let first = usage(200, 200, 150);
		let receipt = |ms, usage: &ThreadTokenUsage| {
			rates.observe_at(
				&ServerEvent::Notification {
					method: "thread/tokenUsage/updated".into(),
					params: json!({"threadId":"child","turnId":"turn","tokenUsage":usage}),
				},
				clock + Duration::from_millis(ms),
				100_000 + ms,
			)
		};
		receipt(120001, &first);
		emit(125000, "item/started", json!({"type":"agentMessage"}));
		assert!(rates.generating("child"));
		emit(126000, "item/completed", json!({"type":"agentMessage"}));
		let second = usage(300, 100, 60);
		receipt(127000, &second);
		let a = rates.take("child", "turn", &first).unwrap();
		assert_eq!(
			(a.output_tokens, a.duration_ms),
			(200, 3500),
			"include generation of parallel tool arguments, exclude their execution"
		);
		let b = rates.take("child", "turn", &second).unwrap();
		assert_eq!((b.output_tokens, b.duration_ms), (40, 1000), "do not count unseen reasoning");
		assert!(rates.take("child", "turn", &second).is_none());
	}
	#[test]
	fn raw_response_finishes_before_tool_wait_and_batched_markers_do_not_invent_speed() {
		let rates = OutputRates::default();
		let clock = Instant::now();
		for (ms, method, params) in [
			(0, "item/started", json!({"item":{"type":"agentMessage"}})),
			(900, "item/completed", json!({"item":{"type":"agentMessage"}})),
			(1000, "rawResponse/completed", json!({"responseId":"r"})),
			(9000, "item/started", json!({"item":{"type":"commandExecution"}})),
		] {
			let mut params = params;
			params["threadId"] = json!("t");
			params["turnId"] = json!("one");
			rates.observe_at(
				&ServerEvent::Notification { method: method.into(), params },
				clock + Duration::from_millis(ms),
				ms,
			);
		}
		assert_eq!(
			rates.0.lock().unwrap().windows["t"]
				.finish(&serde_json::from_value(counts(100, 0)).unwrap())
				.unwrap()
				.duration_ms,
			1000
		);
		let w = Window {
			turn: "one".into(),
			start: clock,
			start_ms: 0,
			end: Some(clock + Duration::from_millis(5)),
			reasoning: false,
			sealed: false,
			tool: false,
			generating: false,
			raw_usage: None,
		};
		assert!(w.finish(&serde_json::from_value(counts(100, 0)).unwrap()).is_none());
		let unknown = Window { end: Some(clock + Duration::from_secs(120)), tool: true, ..w };
		assert!(
			unknown.finish(&serde_json::from_value(counts(100, 0)).unwrap()).is_none(),
			"approval/tool time is not a generation estimate without the native boundary"
		);
		rates.observe_at(
			&ServerEvent::Notification {
				method: "thread/reverted".into(),
				params: json!({"threadId":"t"}),
			},
			clock,
			0,
		);
		assert!(rates.0.lock().unwrap().windows.is_empty());
	}
}
