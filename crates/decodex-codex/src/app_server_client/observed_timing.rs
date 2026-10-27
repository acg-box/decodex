//! Local timing of visible native activity. These observations are not a critical path.

use std::{
	collections::{HashMap, VecDeque},
	sync::{Arc, Mutex},
	time::Instant,
};

use serde::{Deserialize, Serialize};

use super::{MAX_BUFFERED_EVENTS, RequestId, ServerEvent};

/// Completed intervals observed on one transport for one native turn.
/// Missing values are unknown; categories can overlap and do not sum to turn time.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct ObservedTurnTiming {
	/// Visible model output or reasoning item lifetimes, not complete model inference.
	pub model_output_ms: Option<u64>,
	/// Observed native context-compaction item lifetimes.
	pub compaction_ms: Option<u64>,
	/// Observed tool item lifetimes, which can include approval or other waits.
	pub tool_activity_ms: Option<u64>,
	/// Observed native automatic-review lifetimes.
	pub auto_review_ms: Option<u64>,
	/// Observed blocking approval or input requests until response or resolution.
	pub user_request_ms: Option<u64>,
}

#[derive(Clone, Default)]
pub(super) struct ObservedTimings(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
	turns: HashMap<String, Turn>,
	requests: HashMap<RequestId, Request>,
	completed: VecDeque<Receipt>,
}

struct Turn {
	id: String,
	started: bool,
	open: HashMap<(Kind, String), Instant>,
	spans: [Vec<(Instant, Instant)>; 5],
	incomplete: [bool; 5],
}

struct Request {
	thread: String,
	turn: String,
	started: Instant,
}

struct Receipt {
	thread: String,
	turn: String,
	timing: ObservedTurnTiming,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum Kind {
	ModelOutput,
	Compaction,
	Tool,
	Review,
	UserRequest,
}

impl Kind {
	fn index(self) -> usize {
		match self {
			Self::ModelOutput => 0,
			Self::Compaction => 1,
			Self::Tool => 2,
			Self::Review => 3,
			Self::UserRequest => 4,
		}
	}
}

impl Turn {
	fn new(id: String, started: bool) -> Self {
		Self {
			id,
			started,
			open: HashMap::new(),
			spans: std::array::from_fn(|_| Vec::new()),
			incomplete: [false; 5],
		}
	}

	fn start(&mut self, kind: Kind, id: &str, now: Instant) {
		if self.open.len() < MAX_BUFFERED_EVENTS {
			self.open.entry((kind, id.into())).or_insert(now);
		} else {
			self.incomplete[kind.index()] = true;
		}
	}

	fn finish(&mut self, kind: Kind, id: &str, now: Instant) {
		if let Some(start) = self.open.remove(&(kind, id.into())) {
			self.record(kind, start, now);
		} else {
			self.incomplete[kind.index()] = true;
		}
	}

	fn record(&mut self, kind: Kind, start: Instant, end: Instant) {
		let spans = &mut self.spans[kind.index()];

		if end >= start && spans.len() < MAX_BUFFERED_EVENTS {
			spans.push((start, end));
		} else {
			self.incomplete[kind.index()] = true;
		}
	}

	fn timing(mut self) -> ObservedTurnTiming {
		if !self.started {
			return ObservedTurnTiming::default();
		}
		for (kind, _) in self.open.keys() {
			self.incomplete[kind.index()] = true;
		}
		let mut duration = |kind: Kind| {
			(!self.incomplete[kind.index()])
				.then(|| union_ms(&mut self.spans[kind.index()]))
				.flatten()
		};
		ObservedTurnTiming {
			model_output_ms: duration(Kind::ModelOutput),
			compaction_ms: duration(Kind::Compaction),
			tool_activity_ms: duration(Kind::Tool),
			auto_review_ms: duration(Kind::Review),
			user_request_ms: duration(Kind::UserRequest),
		}
	}
}

fn union_ms(spans: &mut [(Instant, Instant)]) -> Option<u64> {
	spans.sort_unstable_by_key(|span| span.0);
	let &(mut start, mut end) = spans.first()?;
	let mut total = 0_u128;

	for &(next_start, next_end) in &spans[1..] {
		if next_start <= end {
			end = end.max(next_end);
		} else {
			total = total.checked_add(end.duration_since(start).as_millis())?;
			(start, end) = (next_start, next_end);
		}
	}

	u64::try_from(total.checked_add(end.duration_since(start).as_millis())?).ok()
}

impl State {
	fn turn(&mut self, thread: &str, turn: &str) -> Option<&mut Turn> {
		if !self.turns.contains_key(thread) && self.turns.len() < MAX_BUFFERED_EVENTS {
			self.turns.insert(thread.into(), Turn::new(turn.into(), false));
		}

		self.turns.get_mut(thread).filter(|active| active.id == turn)
	}

	fn finish_request(&mut self, id: &RequestId, now: Instant) {
		if let Some(request) = self.requests.remove(id)
			&& let Some(turn) = self.turns.get_mut(&request.thread).filter(|t| t.id == request.turn)
		{
			turn.record(Kind::UserRequest, request.started, now);
		}
	}
}

impl ObservedTimings {
	pub(super) fn observe(&self, event: &ServerEvent) {
		match event {
			ServerEvent::Request { method, params, .. } if blocking_request(method, params) => {},
			ServerEvent::Notification { method, .. }
				if matches!(
					method.as_str(),
					"turn/started"
						| "turn/completed"
						| "item/started"
						| "item/completed"
						| "item/autoApprovalReview/started"
						| "item/autoApprovalReview/completed"
						| "serverRequest/resolved"
						| "thread/closed"
						| "thread/archived"
						| "thread/deleted"
						| "thread/reverted"
				) => {},
			_ => return,
		}
		self.observe_at(event, Instant::now());
	}

	fn observe_at(&self, event: &ServerEvent, now: Instant) {
		let Ok(mut state) = self.0.lock() else { return };

		match event {
			ServerEvent::Request { id, method, params } if blocking_request(method, params) => {
				let (Some(thread), Some(turn)) =
					(identity(&params["threadId"]), identity(&params["turnId"]))
				else {
					return;
				};
				if state.requests.len() < MAX_BUFFERED_EVENTS && state.turn(thread, turn).is_some()
				{
					state.requests.entry(id.clone()).or_insert_with(|| Request {
						thread: thread.into(),
						turn: turn.into(),
						started: now,
					});
				} else if let Some(active) = state.turn(thread, turn) {
					active.incomplete[Kind::UserRequest.index()] = true;
				}
			},
			ServerEvent::Notification { method, params } => {
				let Some(thread) = identity(&params["threadId"]) else { return };

				match method.as_str() {
					"turn/started" => {
						let Some(turn) = identity(&params["turn"]["id"]) else { return };
						if state.turns.get(thread).is_some_and(|active| active.id != turn) {
							state.turns.remove(thread);
							state.requests.retain(|_, request| request.thread != thread);
						}
						if !state.turns.contains_key(thread)
							&& state.turns.len() < MAX_BUFFERED_EVENTS
						{
							state.turns.insert(thread.into(), Turn::new(turn.into(), true));
						}
					},
					"turn/completed" => {
						let Some(turn) = identity(&params["turn"]["id"]) else { return };
						if state.turns.get(thread).is_some_and(|active| active.id == turn)
							&& let Some(mut active) = state.turns.remove(thread)
						{
							if state
								.requests
								.values()
								.any(|request| request.thread == thread && request.turn == turn)
							{
								active.incomplete[Kind::UserRequest.index()] = true;
							}
							let timing = active.timing();
							state.requests.retain(|_, request| {
								!(request.thread == thread && request.turn == turn)
							});
							if state.completed.len() == MAX_BUFFERED_EVENTS {
								state.completed.pop_front();
							}
							state.completed.push_back(Receipt {
								thread: thread.into(),
								turn: turn.into(),
								timing,
							});
						}
					},
					"item/started" | "item/completed" => {
						let (Some(turn), Some(id), Some(kind)) = (
							identity(&params["turnId"]),
							identity(&params["item"]["id"]),
							item_kind(&params["item"]),
						) else {
							return;
						};
						if let Some(active) = state.turn(thread, turn) {
							if method == "item/started" {
								active.start(kind, id, now);
							} else {
								active.finish(kind, id, now);
							}
						}
					},
					"item/autoApprovalReview/started" | "item/autoApprovalReview/completed" => {
						let (Some(turn), Some(id)) =
							(identity(&params["turnId"]), identity(&params["reviewId"]))
						else {
							return;
						};
						if let Some(active) = state.turn(thread, turn) {
							if method.ends_with("/started") {
								active.start(Kind::Review, id, now);
							} else {
								active.finish(Kind::Review, id, now);
							}
						}
					},
					"serverRequest/resolved" => {
						if let Ok(id) =
							serde_json::from_value::<RequestId>(params["requestId"].clone())
							&& state
								.requests
								.get(&id)
								.is_some_and(|request| request.thread == thread)
						{
							state.finish_request(&id, now);
						}
					},
					"thread/closed" | "thread/archived" | "thread/deleted" | "thread/reverted" => {
						state.turns.remove(thread);
						state.requests.retain(|_, request| request.thread != thread);
						if method == "thread/reverted" {
							state.completed.retain(|receipt| receipt.thread != thread);
						}
					},
					_ => {},
				}
			},
			_ => {},
		}
	}

	pub(super) fn response_written(&self, id: &RequestId) {
		self.response_written_at(id, Instant::now());
	}

	fn response_written_at(&self, id: &RequestId, now: Instant) {
		if let Ok(mut state) = self.0.lock() {
			state.finish_request(id, now);
		}
	}

	pub(super) fn take(&self, thread: &str, turn: &str) -> Option<ObservedTurnTiming> {
		let mut state = self.0.lock().ok()?;
		let index = state.completed.iter().position(|r| r.thread == thread && r.turn == turn)?;

		Some(state.completed.remove(index)?.timing)
	}

	pub(super) fn clear(&self) {
		if let Ok(mut state) = self.0.lock() {
			// A completed event can already be queued for its owner when the
			// transport closes. Retain its bounded receipt until consumed.
			state.turns.clear();
			state.requests.clear();
		}
	}
}

impl super::AppServerClient {
	/// Consume the local observation for one completed native turn, if retained.
	pub fn take_observed_turn_timing(
		&self,
		thread: &str,
		turn: &str,
	) -> Option<ObservedTurnTiming> {
		self.server_requests.take_timing(thread, turn)
	}
}

fn identity(value: &serde_json::Value) -> Option<&str> {
	value.as_str().filter(|value| !value.is_empty() && value.len() <= 512)
}

fn blocking_request(method: &str, params: &serde_json::Value) -> bool {
	matches!(
		method,
		"item/commandExecution/requestApproval"
			| "item/fileChange/requestApproval"
			| "item/permissions/requestApproval"
			| "mcpServer/elicitation/request"
	) || (method == "item/tool/requestUserInput"
		&& matches!(
			params.get("isBlocking"),
			None | Some(serde_json::Value::Null | serde_json::Value::Bool(true))
		))
}

fn item_kind(item: &serde_json::Value) -> Option<Kind> {
	match item["type"].as_str()? {
		"reasoning" | "agentMessage" | "plan" if item["delivery"] != "async" =>
			Some(Kind::ModelOutput),
		"contextCompaction" => Some(Kind::Compaction),
		"commandExecution"
		| "dynamicToolCall"
		| "mcpToolCall"
		| "collabAgentToolCall"
		| "webSearch"
		| "fileChange"
		| "imageGeneration"
		| "imageView"
		| "sleep" => Some(Kind::Tool),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use super::{ObservedTimings, ObservedTurnTiming, RequestId, ServerEvent};

	fn notification(method: &str, params: serde_json::Value) -> ServerEvent {
		ServerEvent::Notification { method: method.into(), params }
	}

	#[test]
	fn overlapping_observed_intervals_are_unioned_and_kept_in_separate_categories() {
		let timings = ObservedTimings::default();
		let start = Instant::now();
		let emit = |ms, method: &str, params| {
			timings.observe_at(&notification(method, params), start + Duration::from_millis(ms))
		};
		emit(0, "turn/started", serde_json::json!({"threadId":"thread","turn":{"id":"turn"}}));
		for (ms, method, id, kind) in [
			(1_000, "item/started", "model", "agentMessage"),
			(2_000, "item/started", "a", "mcpToolCall"),
			(4_000, "item/started", "b", "commandExecution"),
			(6_000, "item/completed", "model", "agentMessage"),
			(8_000, "item/completed", "a", "mcpToolCall"),
			(9_000, "item/completed", "b", "commandExecution"),
		] {
			emit(
				ms,
				method,
				serde_json::json!({"threadId":"thread","turnId":"turn","item":{"id":id,"type":kind}}),
			);
		}
		emit(
			3_000,
			"item/autoApprovalReview/started",
			serde_json::json!({"threadId":"thread","turnId":"turn","reviewId":"review"}),
		);
		emit(
			4_000,
			"item/autoApprovalReview/completed",
			serde_json::json!({"threadId":"thread","turnId":"turn","reviewId":"review"}),
		);
		let request = ServerEvent::Request {
			id: RequestId::Number(7),
			method: "item/tool/requestUserInput".into(),
			params: serde_json::json!({"threadId":"thread","turnId":"turn","isBlocking":true}),
		};
		timings.observe_at(&request, start + Duration::from_millis(4_000));
		timings.response_written_at(&RequestId::Number(7), start + Duration::from_millis(7_000));
		emit(
			10_000,
			"turn/completed",
			serde_json::json!({"threadId":"thread","turn":{"id":"turn"}}),
		);

		assert_eq!(
			timings.take("thread", "turn"),
			Some(ObservedTurnTiming {
				model_output_ms: Some(5_000),
				compaction_ms: None,
				tool_activity_ms: Some(7_000),
				auto_review_ms: Some(1_000),
				user_request_ms: Some(3_000),
			})
		);
		assert_eq!(timings.take("thread", "turn"), None, "a receipt is consumed once");
	}

	#[test]
	fn missing_boundaries_and_nonblocking_questions_remain_unknown() {
		let timings = ObservedTimings::default();
		let start = Instant::now();
		timings.observe_at(
			&notification("turn/started", serde_json::json!({"threadId":"t","turn":{"id":"u"}})),
			start,
		);
		timings.observe_at(&notification("item/completed", serde_json::json!({"threadId":"t","turnId":"u","item":{"id":"missing-start","type":"commandExecution"}})), start + Duration::from_secs(1));
		timings.observe_at(
			&ServerEvent::Request {
				id: RequestId::Number(9),
				method: "item/tool/requestUserInput".into(),
				params: serde_json::json!({"threadId":"t","turnId":"u","isBlocking":false}),
			},
			start,
		);
		timings.response_written_at(&RequestId::Number(9), start + Duration::from_secs(2));
		timings.observe_at(
			&notification("turn/completed", serde_json::json!({"threadId":"t","turn":{"id":"u"}})),
			start + Duration::from_secs(3),
		);
		assert_eq!(timings.take("t", "u"), Some(ObservedTurnTiming::default()));
	}

	#[test]
	fn legacy_question_without_blocking_field_is_measured() {
		let timings = ObservedTimings::default();
		let start = Instant::now();
		timings.observe_at(
			&notification("turn/started", serde_json::json!({"threadId":"t","turn":{"id":"u"}})),
			start,
		);
		timings.observe_at(
			&ServerEvent::Request {
				id: RequestId::Number(1),
				method: "item/tool/requestUserInput".into(),
				params: serde_json::json!({"threadId":"t","turnId":"u"}),
			},
			start,
		);
		timings.response_written_at(&RequestId::Number(1), start + Duration::from_secs(2));
		timings.observe_at(
			&notification("turn/completed", serde_json::json!({"threadId":"t","turn":{"id":"u"}})),
			start + Duration::from_secs(3),
		);
		assert_eq!(timings.take("t", "u").unwrap().user_request_ms, Some(2_000));
	}

	#[test]
	fn incomplete_intervals_make_their_whole_category_unknown() {
		let timings = ObservedTimings::default();
		let start = Instant::now();
		let emit = |ms, method: &str, params| {
			timings.observe_at(&notification(method, params), start + Duration::from_millis(ms))
		};
		emit(0, "turn/started", serde_json::json!({"threadId":"t","turn":{"id":"u"}}));
		for (ms, method, id, kind) in [
			(1, "item/started", "complete", "commandExecution"),
			(2, "item/completed", "complete", "commandExecution"),
			(3, "item/started", "unfinished", "commandExecution"),
			(4, "item/started", "model", "agentMessage"),
			(6, "item/completed", "model", "agentMessage"),
		] {
			emit(
				ms,
				method,
				serde_json::json!({"threadId":"t","turnId":"u","item":{"id":id,"type":kind}}),
			);
		}
		for (id, at) in [(1, 1), (2, 3)] {
			timings.observe_at(
				&ServerEvent::Request {
					id: RequestId::Number(id),
					method: "item/commandExecution/requestApproval".into(),
					params: serde_json::json!({"threadId":"t","turnId":"u"}),
				},
				start + Duration::from_millis(at),
			);
		}
		timings.response_written_at(&RequestId::Number(1), start + Duration::from_millis(2));
		emit(7, "turn/completed", serde_json::json!({"threadId":"t","turn":{"id":"u"}}));
		let observed = timings.take("t", "u").unwrap();
		assert_eq!(observed.tool_activity_ms, None);
		assert_eq!(observed.user_request_ms, None);
		assert_eq!(observed.model_output_ms, Some(2));
	}

	#[test]
	fn request_resolution_is_thread_bound_and_reverts_invalidate_pending_receipts() {
		let timings = ObservedTimings::default();
		let start = Instant::now();
		let request = ServerEvent::Request {
			id: RequestId::String("request".into()),
			method: "item/commandExecution/requestApproval".into(),
			params: serde_json::json!({"threadId":"owner","turnId":"turn"}),
		};
		timings.observe_at(
			&notification(
				"turn/started",
				serde_json::json!({"threadId":"owner","turn":{"id":"turn"}}),
			),
			start,
		);
		timings.observe_at(&request, start);
		timings.observe_at(
			&notification(
				"serverRequest/resolved",
				serde_json::json!({"threadId":"other","requestId":"request"}),
			),
			start + Duration::from_secs(1),
		);
		timings.observe_at(
			&notification(
				"serverRequest/resolved",
				serde_json::json!({"threadId":"owner","requestId":"request"}),
			),
			start + Duration::from_secs(3),
		);
		timings.observe_at(
			&notification(
				"turn/completed",
				serde_json::json!({"threadId":"owner","turn":{"id":"turn"}}),
			),
			start + Duration::from_secs(4),
		);
		assert_eq!(timings.take("other", "turn"), None);
		assert_eq!(timings.take("owner", "turn").unwrap().user_request_ms, Some(3_000));

		timings.observe_at(
			&notification(
				"turn/started",
				serde_json::json!({"threadId":"owner","turn":{"id":"turn"}}),
			),
			start + Duration::from_secs(5),
		);
		timings.observe_at(&request, start + Duration::from_secs(5));
		timings.observe_at(
			&notification(
				"turn/completed",
				serde_json::json!({"threadId":"owner","turn":{"id":"turn"}}),
			),
			start + Duration::from_secs(6),
		);
		timings.observe_at(
			&notification("thread/reverted", serde_json::json!({"threadId":"owner"})),
			start + Duration::from_secs(7),
		);
		assert_eq!(timings.take("owner", "turn"), None);
	}

	#[test]
	fn attaching_mid_turn_never_claims_complete_category_totals() {
		let timings = ObservedTimings::default();
		let start = Instant::now();
		for (ms, method) in [(0, "item/started"), (1_000, "item/completed")] {
			timings.observe_at(
				&notification(method, serde_json::json!({"threadId":"t","turnId":"u","item":{"id":"tool","type":"commandExecution"}})),
				start + Duration::from_millis(ms),
			);
		}
		timings.observe_at(
			&notification("turn/completed", serde_json::json!({"threadId":"t","turn":{"id":"u"}})),
			start + Duration::from_secs(2),
		);
		assert_eq!(timings.take("t", "u"), Some(ObservedTurnTiming::default()));
	}

	#[tokio::test]
	async fn guarded_response_records_only_after_transport_writes_it() {
		let (local, remote) = io::duplex(8_192);
		let (reader, writer) = io::split(local);
		let (client, mut events) = super::super::AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for frame in [
				serde_json::json!({"method":"turn/started","params":{"threadId":"thread","turn":{"id":"turn"}}}),
				serde_json::json!({"id":7,"method":"item/tool/requestUserInput","params":{"threadId":"thread","turnId":"turn","isBlocking":true}}),
			] {
				writer.write_all(format!("{frame}\n").as_bytes()).await.unwrap();
			}
			let response: serde_json::Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(response["id"], 7);
			assert_eq!(response["result"]["answer"], "fixture");
			let completed = serde_json::json!({"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"turn"}}});
			writer.write_all(format!("{completed}\n").as_bytes()).await.unwrap();
		});

		assert!(matches!(events.recv().await, Some(ServerEvent::Notification { .. })));
		let Some(ServerEvent::Request { id, method, params }) = events.recv().await else {
			panic!("expected request")
		};
		let guard = client.server_request_guard(&id, &method, &params).unwrap();
		client.respond_guarded(id, serde_json::json!({"answer":"fixture"}), guard).await.unwrap();
		assert!(matches!(events.recv().await, Some(ServerEvent::Notification { .. })));
		let observed = client.take_observed_turn_timing("thread", "turn").unwrap();
		assert!(observed.user_request_ms.is_some());
		assert_eq!(observed.tool_activity_ms, None);
		server.await.unwrap();
	}
}
