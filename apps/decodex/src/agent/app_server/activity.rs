use std::{
	collections::HashMap,
	time::{Duration, Instant},
};

use serde_json::{self, Value};
use time::OffsetDateTime;

use crate::state;

use super::{MODEL_EXECUTION_IDLE_TIMEOUT, interactive_flag_for_request};

const CHILD_BUCKET_MODEL: &str = "Model";
const WAITING_REASON_MODEL_EXECUTION: &str = "model_execution";
const CHILD_BUCKET_PROTOCOL: &str = "Protocol";
const CHILD_BUCKET_TOOL: &str = "Tool";
const CHILD_BUCKET_SHELL: &str = "Shell";
const CHILD_BUCKET_TRACKER: &str = "Tracker";
const CHILD_BUCKET_BROWSER_IMAGE: &str = "Browser/Image";
const CHILD_BUCKET_PR_LAND: &str = "PR/Land";
const LARGE_CHILD_OUTPUT_BYTES: i64 = 100_000;
const RECENT_PROTOCOL_ACTIVITY_LIMIT: usize = 8;
const INPUT_TOKEN_KEYS: &[&str] = &[
	"input_tokens",
	"inputTokens",
	"prompt_tokens",
	"promptTokens",
	"total_input_tokens",
	"totalInputTokens",
];
const OUTPUT_TOKEN_KEYS: &[&str] = &[
	"output_tokens",
	"outputTokens",
	"completion_tokens",
	"completionTokens",
	"total_output_tokens",
	"totalOutputTokens",
];

#[derive(Clone, Debug)]
struct ChildActivityEvent {
	event_bucket: String,
	event_detail: Option<String>,
	transition_bucket: Option<String>,
	transition_detail: Option<String>,
	tool_name: Option<String>,
	tool_call: bool,
	tool_output_bytes: Option<i64>,
	input_tokens: Option<i64>,
	output_tokens: Option<i64>,
	completed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct LargeOutputStats {
	count: i64,
	max_bytes: i64,
}

pub(super) struct ChildActivityAccumulator {
	started_at: Instant,
	last_observed_at: Instant,
	current_bucket: Option<String>,
	current_detail: Option<String>,
	active_tool_name: Option<String>,
	large_output_stats: HashMap<String, LargeOutputStats>,
	summary: state::ChildAgentActivitySummary,
}
impl ChildActivityAccumulator {
	pub(super) fn new() -> Self {
		let now = Instant::now();

		Self {
			started_at: now,
			last_observed_at: now,
			current_bucket: None,
			current_detail: None,
			active_tool_name: None,
			large_output_stats: HashMap::new(),
			summary: state::ChildAgentActivitySummary::default(),
		}
	}

	pub(super) fn record(
		&mut self,
		event_type: &str,
		payload: &str,
	) -> state::ChildAgentActivitySummary {
		let now = Instant::now();
		let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

		self.add_elapsed_time(now);

		let event =
			classify_child_activity_event(event_type, payload, self.active_tool_name.as_deref());

		self.summary.event_count += 1;
		self.summary.wall_seconds =
			duration_seconds_i64(now.saturating_duration_since(self.started_at));

		self.record_event_bucket(&event);

		if let Some(tool_name) = event.tool_name.as_ref().filter(|_tool_name| event.tool_call) {
			self.active_tool_name = Some(tool_name.clone());
		}

		if event_type == "item/tool/call/response" {
			self.active_tool_name = None;
		}
		if event.completed {
			self.set_current(None, None, None);
		} else if let Some(next_bucket) = event.transition_bucket {
			self.set_current(Some(next_bucket), event.transition_detail, Some(now_unix_epoch));
		}

		self.summary.current_elapsed_seconds =
			self.summary.current_started_unix_epoch.and_then(|started_at| {
				now_unix_epoch.checked_sub(started_at).filter(|elapsed| *elapsed >= 0)
			});
		self.last_observed_at = now;

		self.summary.clone()
	}

	fn add_elapsed_time(&mut self, now: Instant) {
		let Some(bucket_name) = self.current_bucket.clone() else {
			return;
		};
		let seconds = duration_seconds_i64(now.saturating_duration_since(self.last_observed_at));
		let bucket = child_activity_bucket_mut(&mut self.summary, &bucket_name);

		bucket.wall_seconds = bucket.wall_seconds.saturating_add(seconds);
	}

	fn record_event_bucket(&mut self, event: &ChildActivityEvent) {
		{
			let bucket = child_activity_bucket_mut(&mut self.summary, &event.event_bucket);

			bucket.event_count += 1;

			if event.tool_call {
				bucket.tool_call_count += 1;
			}

			if let Some(input_tokens) = event.input_tokens {
				bucket.input_tokens = bucket.input_tokens.saturating_add(input_tokens);
			}
			if let Some(output_tokens) = event.output_tokens {
				bucket.output_tokens = bucket.output_tokens.saturating_add(output_tokens);
			}
			if let Some(output_bytes) = event.tool_output_bytes {
				bucket.output_bytes = bucket.output_bytes.saturating_add(output_bytes);
			}
		}

		if event.tool_call {
			self.summary.tool_call_count += 1;
		}

		if let Some(input_tokens) = event.input_tokens {
			self.summary.input_tokens_current = Some(input_tokens);
			self.summary.input_tokens_max = Some(
				self.summary
					.input_tokens_max
					.map_or(input_tokens, |max_tokens| max_tokens.max(input_tokens)),
			);
			self.summary.input_tokens_cumulative =
				self.summary.input_tokens_cumulative.saturating_add(input_tokens);
		}
		if let Some(output_tokens) = event.output_tokens {
			self.summary.output_tokens_cumulative =
				self.summary.output_tokens_cumulative.saturating_add(output_tokens);
		}
		if let Some(output_bytes) = event.tool_output_bytes {
			self.record_tool_output(event, output_bytes);
		}
	}

	fn record_tool_output(&mut self, event: &ChildActivityEvent, output_bytes: i64) {
		let tool_name =
			event.tool_name.as_deref().or(event.event_detail.as_deref()).unwrap_or("tool");

		if self.summary.largest_tool_output_bytes.is_none_or(|largest| output_bytes > largest) {
			self.summary.largest_tool_output_bytes = Some(output_bytes);
			self.summary.largest_tool_output_tool = Some(tool_name.to_owned());
		}
		if output_bytes < LARGE_CHILD_OUTPUT_BYTES {
			return;
		}

		let stats = self.large_output_stats.entry(tool_name.to_owned()).or_default();

		stats.count += 1;
		stats.max_bytes = stats.max_bytes.max(output_bytes);

		self.refresh_large_output_warnings();
	}

	fn refresh_large_output_warnings(&mut self) {
		let mut entries = self
			.large_output_stats
			.iter()
			.map(|(tool_name, stats)| (tool_name.clone(), *stats))
			.collect::<Vec<_>>();

		entries.sort_by(|left, right| {
			right.1.max_bytes.cmp(&left.1.max_bytes).then_with(|| left.0.cmp(&right.0))
		});

		self.summary.large_output_warnings = entries
			.into_iter()
			.take(4)
			.map(|(tool_name, stats)| {
				if stats.count > 1 {
					format!(
						"{tool_name} repeated {} large outputs; largest {} bytes",
						stats.count, stats.max_bytes
					)
				} else {
					format!("{tool_name} produced a large output: {} bytes", stats.max_bytes)
				}
			})
			.collect();
	}

	fn set_current(
		&mut self,
		bucket: Option<String>,
		detail: Option<String>,
		started_unix_epoch: Option<i64>,
	) {
		if self.current_bucket == bucket && self.current_detail == detail {
			return;
		}

		self.current_bucket = bucket.clone();
		self.current_detail = detail.clone();
		self.summary.current_bucket = bucket;
		self.summary.current_detail = detail;
		self.summary.current_started_unix_epoch = started_unix_epoch;
		self.summary.current_elapsed_seconds = None;
	}
}

pub(super) struct ProtocolActivityAccumulator {
	pub(super) summary: state::ProtocolActivitySummary,
}
impl ProtocolActivityAccumulator {
	pub(super) fn new() -> Self {
		Self { summary: state::ProtocolActivitySummary::default() }
	}

	pub(super) fn record(
		&mut self,
		event_type: &str,
		payload: &str,
		child_activity: &state::ChildAgentActivitySummary,
	) -> state::ProtocolActivitySummary {
		self.summary.recent_events.push(protocol_activity_event(event_type, payload));

		if self.summary.recent_events.len() > RECENT_PROTOCOL_ACTIVITY_LIMIT {
			let remove_count =
				self.summary.recent_events.len().saturating_sub(RECENT_PROTOCOL_ACTIVITY_LIMIT);

			self.summary.recent_events.drain(0..remove_count);
		}

		if let Some(turn_status) = protocol_turn_status_from_payload(event_type, payload) {
			self.summary.turn_status = Some(turn_status);
		}
		if let Some(waiting_reason) = protocol_waiting_reason(event_type, payload, child_activity) {
			self.summary.waiting_reason = Some(waiting_reason);
		}
		if let Some(rate_limit_status) = protocol_rate_limit_status(event_type, payload) {
			self.summary.rate_limit_status = Some(rate_limit_status);
		}

		self.summary.clone()
	}
}

pub(crate) fn protocol_activity_idle_timeout(
	protocol_activity: Option<&state::ProtocolActivitySummary>,
	base_timeout: Duration,
) -> Duration {
	if protocol_activity.is_some_and(running_model_execution_protocol_activity) {
		return base_timeout.max(MODEL_EXECUTION_IDLE_TIMEOUT);
	}

	base_timeout
}

fn running_model_execution_protocol_activity(
	protocol_activity: &state::ProtocolActivitySummary,
) -> bool {
	protocol_activity.turn_status.as_deref() == Some("running")
		&& protocol_activity.waiting_reason.as_deref() == Some(WAITING_REASON_MODEL_EXECUTION)
}

fn classify_child_activity_event(
	event_type: &str,
	payload: &str,
	active_tool_name: Option<&str>,
) -> ChildActivityEvent {
	let payload_value = serde_json::from_str::<Value>(payload).ok();
	let input_tokens =
		payload_value.as_ref().and_then(|value| find_numeric_field(value, INPUT_TOKEN_KEYS));
	let output_tokens =
		payload_value.as_ref().and_then(|value| find_numeric_field(value, OUTPUT_TOKEN_KEYS));

	match event_type {
		"item/tool/call" => {
			child_tool_call_event(payload_value.as_ref(), input_tokens, output_tokens)
		},
		"item/tool/call/response" => {
			child_tool_response_event(payload_value.as_ref(), active_tool_name, payload)
		},
		"item/completed" => child_item_completed_event(payload_value.as_ref(), payload),
		"item/agentMessage/delta" => ChildActivityEvent {
			event_bucket: CHILD_BUCKET_MODEL.to_owned(),
			event_detail: Some(String::from("agent_message_delta")),
			transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
			transition_detail: Some(String::from("streaming response")),
			tool_name: None,
			tool_call: false,
			tool_output_bytes: None,
			input_tokens,
			output_tokens,
			completed: false,
		},
		"turn/completed" => ChildActivityEvent {
			event_bucket: CHILD_BUCKET_MODEL.to_owned(),
			event_detail: Some(String::from("turn_completed")),
			transition_bucket: None,
			transition_detail: None,
			tool_name: None,
			tool_call: false,
			tool_output_bytes: None,
			input_tokens,
			output_tokens,
			completed: true,
		},
		"thread/status/changed" => ChildActivityEvent {
			event_bucket: CHILD_BUCKET_MODEL.to_owned(),
			event_detail: Some(String::from("thread_status")),
			transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
			transition_detail: Some(String::from("child thread active")),
			tool_name: None,
			tool_call: false,
			tool_output_bytes: None,
			input_tokens,
			output_tokens,
			completed: false,
		},
		other => ChildActivityEvent {
			event_bucket: CHILD_BUCKET_PROTOCOL.to_owned(),
			event_detail: Some(other.to_owned()),
			transition_bucket: None,
			transition_detail: None,
			tool_name: None,
			tool_call: false,
			tool_output_bytes: None,
			input_tokens,
			output_tokens,
			completed: false,
		},
	}
}

fn protocol_activity_event(event_type: &str, payload: &str) -> state::ProtocolActivityEventSummary {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	state::ProtocolActivityEventSummary {
		event_type: event_type.to_owned(),
		category: protocol_activity_category(event_type).to_owned(),
		detail: protocol_activity_detail(event_type, payload_value.as_ref()),
	}
}

fn protocol_activity_category(event_type: &str) -> &'static str {
	let normalized = event_type.to_ascii_lowercase();

	if normalized.starts_with("turn/") {
		return "turn";
	}
	if normalized.contains("plan") {
		return "plan";
	}
	if normalized.contains("diff") || normalized.contains("filechange") {
		return "diff";
	}
	if normalized.contains("command")
		&& (normalized.contains("output") || normalized.contains("delta"))
	{
		return "command_output";
	}
	if normalized.contains("ratelimit") || normalized.contains("rate_limit") {
		return "rate_limit";
	}
	if normalized.starts_with("account/") {
		return "account";
	}
	if normalized == "deprecationnotice" {
		return "deprecation";
	}
	if normalized == "warning" || normalized == "configwarning" || normalized == "guardianwarning" {
		return "warning";
	}
	if normalized.starts_with("model/") {
		return "model";
	}
	if normalized.contains("tokenusage") {
		return "token_usage";
	}
	if normalized.contains("reasoning") {
		return "reasoning";
	}
	if normalized == "item/tool/call/failure" {
		return "protocol_error";
	}
	if normalized.ends_with("/response") || normalized == "json-rpc/error/response" {
		return "server_request_resolution";
	}
	if normalized.starts_with("item/") {
		return "item";
	}
	if normalized == "thread/status/changed" {
		return "thread";
	}
	if normalized == "error" || normalized.contains("error") {
		return "protocol_error";
	}

	"protocol"
}

fn protocol_activity_detail(event_type: &str, payload_value: Option<&Value>) -> Option<String> {
	let normalized = event_type.to_ascii_lowercase();

	if matches!(event_type, "thread/goal/set" | "thread/goal/get" | "thread/goal/updated") {
		return phase_goal_activity_detail(payload_value);
	}
	if event_type == "thread/status/changed" {
		return payload_value.and_then(|value| {
			string_at_paths(value, &[&["params", "status", "type"], &["status", "type"]])
		});
	}
	if event_type == "turn/steer" {
		return protocol_steer_detail(payload_value);
	}
	if event_type.starts_with("turn/") {
		return protocol_turn_status_from_value(event_type, payload_value)
			.or_else(|| Some(String::from("running")));
	}
	if event_type == "item/tool/call" {
		return payload_value.and_then(extract_tool_name);
	}
	if event_type == "item/tool/call/failure" {
		return payload_value.and_then(|value| {
			string_at_paths(value, &[&["failureClass"], &["failure_class"]])
				.or_else(|| string_at_paths(value, &[&["tool"]]))
		});
	}
	if event_type == "item/completed" {
		return payload_value.and_then(|value| {
			string_at_paths(value, &[&["params", "item", "type"], &["item", "type"]])
		});
	}
	if event_type.starts_with("account/") {
		return protocol_account_detail(payload_value);
	}
	if normalized == "deprecationnotice"
		|| normalized == "warning"
		|| normalized == "configwarning"
		|| normalized == "guardianwarning"
	{
		return warning_or_deprecation_detail(payload_value);
	}
	if event_type == "model/rerouted" {
		return model_rerouted_detail(payload_value);
	}
	if event_type == "model/verification" {
		return model_verification_detail(payload_value);
	}
	if normalized.contains("tokenusage") {
		return token_usage_detail(payload_value);
	}
	if normalized.contains("reasoning") {
		return payload_value.and_then(|value| {
			string_at_paths(
				value,
				&[
					&["params", "text"],
					&["text"],
					&["params", "summary"],
					&["summary"],
					&["params", "part", "text"],
					&["part", "text"],
				],
			)
		});
	}
	if event_type == "error" {
		return payload_value
			.and_then(|value| {
				string_at_paths(
					value,
					&[&["params", "error", "codexErrorInfo"], &["error", "codexErrorInfo"]],
				)
			})
			.or_else(|| Some(String::from("error")));
	}

	None
}

fn phase_goal_activity_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let status = string_at_paths(
		value,
		&[&["payload", "status"], &["params", "goal", "status"], &["goal", "status"], &["status"]],
	)?;
	let phase = string_at_paths(value, &[&["phase"], &["payload", "phase"]]);

	Some(match phase {
		Some(phase) => format!("{phase}/{status}"),
		None => status,
	})
}

fn protocol_steer_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let outcome =
		string_at_paths(value, &[&["outcome"]]).unwrap_or_else(|| String::from("unknown"));
	let expected_turn_id = string_at_paths(value, &[&["expectedTurnId"], &["expected_turn_id"]])
		.unwrap_or_else(|| String::from("unknown"));
	let response_turn_id = string_at_paths(value, &[&["responseTurnId"], &["response_turn_id"]])
		.unwrap_or_else(|| String::from("none"));

	Some(format!("{outcome}: expected={expected_turn_id}, response={response_turn_id}"))
}

fn warning_or_deprecation_detail(payload_value: Option<&Value>) -> Option<String> {
	payload_value.and_then(|value| {
		string_at_paths(
			value,
			&[
				&["params", "summary"],
				&["summary"],
				&["params", "message"],
				&["message"],
				&["params", "details"],
				&["details"],
			],
		)
	})
}

fn model_rerouted_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let from_model = string_at_paths(value, &[&["params", "fromModel"], &["fromModel"]])?;
	let to_model = string_at_paths(value, &[&["params", "toModel"], &["toModel"]])?;
	let reason = string_at_paths(value, &[&["params", "reason"], &["reason"]]);

	Some(match reason {
		Some(reason) => format!("{from_model}->{to_model}/{reason}"),
		None => format!("{from_model}->{to_model}"),
	})
}

fn model_verification_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let verifications = value_at_paths(value, &[&["params", "verifications"], &["verifications"]])?;
	let verification_count = verifications.as_array()?.len();

	Some(format!("{verification_count} verification(s)"))
}

fn token_usage_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let input_tokens = value_at_paths(
		value,
		&[
			&["params", "tokenUsage", "total", "inputTokens"],
			&["tokenUsage", "total", "inputTokens"],
		],
	)
	.and_then(json_number_to_i64);
	let output_tokens = value_at_paths(
		value,
		&[
			&["params", "tokenUsage", "total", "outputTokens"],
			&["tokenUsage", "total", "outputTokens"],
		],
	)
	.and_then(json_number_to_i64);

	match (input_tokens, output_tokens) {
		(Some(input_tokens), Some(output_tokens)) => {
			Some(format!("input={input_tokens}, output={output_tokens}"))
		},
		(Some(input_tokens), None) => Some(format!("input={input_tokens}")),
		(None, Some(output_tokens)) => Some(format!("output={output_tokens}")),
		(None, None) => None,
	}
}

fn protocol_account_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let plan = string_at_paths(
		value,
		&[
			&["params", "planType"],
			&["params", "chatgptPlanType"],
			&["params", "rateLimits", "planType"],
			&["planType"],
			&["chatgptPlanType"],
			&["rateLimits", "planType"],
		],
	);
	let status = string_at_paths(
		value,
		&[
			&["params", "status"],
			&["params", "refreshStatus"],
			&["params", "rateLimits", "rateLimitReachedType"],
			&["status"],
			&["refreshStatus"],
			&["rateLimits", "rateLimitReachedType"],
		],
	);

	match (plan, status) {
		(Some(plan), Some(status)) => Some(format!("{plan}/{status}")),
		(Some(plan), None) => Some(plan),
		(None, Some(status)) => Some(status),
		(None, None) => None,
	}
}

fn protocol_turn_status_from_payload(event_type: &str, payload: &str) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	protocol_turn_status_from_value(event_type, payload_value.as_ref())
}

fn protocol_turn_status_from_value(
	event_type: &str,
	payload_value: Option<&Value>,
) -> Option<String> {
	match event_type {
		"turn/started" => Some(String::from("running")),
		"turn/completed" => payload_value
			.and_then(|value| {
				string_at_paths(value, &[&["params", "turn", "status"], &["turn", "status"]])
			})
			.or_else(|| Some(String::from("completed"))),
		_ => None,
	}
}

fn protocol_waiting_reason(
	event_type: &str,
	payload: &str,
	child_activity: &state::ChildAgentActivitySummary,
) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	if event_type == "thread/status/changed"
		&& let Some(reason) = thread_status_waiting_reason(payload_value.as_ref())
	{
		return Some(reason);
	}
	if interactive_flag_for_request(event_type).is_some() {
		return Some(String::from("approval_or_user_input"));
	}
	if event_type == "item/tool/call" {
		return Some(String::from("tool_execution"));
	}
	if protocol_activity_category(event_type) == "command_output" {
		return Some(String::from("tool_execution"));
	}
	if matches!(event_type, "item/tool/call/response" | "item/completed" | "turn/started")
		|| event_type.ends_with("/delta")
		|| event_type.ends_with("/response")
	{
		return Some(String::from("model_execution"));
	}
	if event_type == "turn/completed" {
		return Some(String::from("turn_completed"));
	}

	if let Some(current_bucket) = child_activity.current_bucket.as_deref() {
		return Some(match current_bucket {
			CHILD_BUCKET_MODEL => String::from("model_execution"),
			CHILD_BUCKET_PROTOCOL => String::from("protocol_activity"),
			_ => String::from("tool_execution"),
		});
	}

	None
}

fn thread_status_waiting_reason(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let flags =
		value_at_paths(value, &[&["params", "status", "activeFlags"], &["status", "activeFlags"]])?;
	let flags = flags.as_array()?;

	if flags
		.iter()
		.filter_map(Value::as_str)
		.any(|flag| matches!(flag, "waitingOnApproval" | "waitingOnUserInput"))
	{
		return Some(String::from("approval_or_user_input"));
	}

	None
}

fn protocol_rate_limit_status(_event_type: &str, payload: &str) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok()?;

	find_string_field(&payload_value, &["rateLimitReachedType", "rate_limit_reached_type"]).or_else(
		|| {
			find_string_field(&payload_value, &["codexErrorInfo", "codex_error_info"])
				.filter(|value| value.to_ascii_lowercase().contains("limit"))
		},
	)
}

fn child_tool_call_event(
	payload_value: Option<&Value>,
	input_tokens: Option<i64>,
	output_tokens: Option<i64>,
) -> ChildActivityEvent {
	let tool_name =
		payload_value.and_then(extract_tool_name).unwrap_or_else(|| String::from("tool"));
	let arguments = payload_value.and_then(extract_tool_arguments);
	let (bucket, detail) = child_tool_bucket(&tool_name, arguments.as_ref());

	ChildActivityEvent {
		event_bucket: bucket.clone(),
		event_detail: Some(detail.clone()),
		transition_bucket: Some(bucket),
		transition_detail: Some(detail),
		tool_name: Some(tool_name),
		tool_call: true,
		tool_output_bytes: None,
		input_tokens,
		output_tokens,
		completed: false,
	}
}

fn child_tool_response_event(
	payload_value: Option<&Value>,
	active_tool_name: Option<&str>,
	payload: &str,
) -> ChildActivityEvent {
	let tool_name = active_tool_name.unwrap_or("tool").to_owned();
	let (bucket, detail) = child_tool_bucket(&tool_name, None);
	let output_bytes = tool_output_size(payload_value, payload);

	ChildActivityEvent {
		event_bucket: bucket,
		event_detail: Some(detail),
		transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
		transition_detail: Some(String::from("waiting after tool output")),
		tool_name: Some(tool_name),
		tool_call: false,
		tool_output_bytes: Some(output_bytes),
		input_tokens: payload_value.and_then(|value| find_numeric_field(value, INPUT_TOKEN_KEYS)),
		output_tokens: payload_value.and_then(|value| find_numeric_field(value, OUTPUT_TOKEN_KEYS)),
		completed: false,
	}
}

fn child_item_completed_event(payload_value: Option<&Value>, payload: &str) -> ChildActivityEvent {
	let item_kind = payload_value
		.and_then(|value| string_at_paths(value, &[&["params", "item", "type"], &["item", "type"]]))
		.unwrap_or_else(|| String::from("item"));
	let tool_name = payload_value.and_then(extract_tool_name);
	let input_tokens = payload_value.and_then(|value| find_numeric_field(value, INPUT_TOKEN_KEYS));
	let output_tokens =
		payload_value.and_then(|value| find_numeric_field(value, OUTPUT_TOKEN_KEYS));

	if let Some(tool_name) = tool_name
		&& item_kind != "agentMessage"
	{
		let (bucket, detail) = child_tool_bucket(&tool_name, None);

		return ChildActivityEvent {
			event_bucket: bucket,
			event_detail: Some(detail),
			transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
			transition_detail: Some(String::from("waiting after completed item")),
			tool_name: Some(tool_name),
			tool_call: false,
			tool_output_bytes: Some(tool_output_size(payload_value, payload)),
			input_tokens,
			output_tokens,
			completed: false,
		};
	}

	ChildActivityEvent {
		event_bucket: CHILD_BUCKET_MODEL.to_owned(),
		event_detail: Some(item_kind),
		transition_bucket: Some(CHILD_BUCKET_MODEL.to_owned()),
		transition_detail: Some(String::from("model output")),
		tool_name: None,
		tool_call: false,
		tool_output_bytes: None,
		input_tokens,
		output_tokens,
		completed: false,
	}
}

fn child_tool_bucket(tool_name: &str, arguments: Option<&Value>) -> (String, String) {
	let normalized_tool = tool_name.to_ascii_lowercase();

	if is_tracker_tool_name(&normalized_tool) {
		return (CHILD_BUCKET_TRACKER.to_owned(), tool_name.to_owned());
	}
	if normalized_tool.contains("view_image")
		|| normalized_tool.contains("screenshot")
		|| normalized_tool.contains("image_query")
		|| normalized_tool.contains("browser")
	{
		return (CHILD_BUCKET_BROWSER_IMAGE.to_owned(), tool_name.to_owned());
	}
	if normalized_tool.contains("exec_command") {
		let command_category = arguments
			.and_then(extract_command_text)
			.map(|command| shell_command_category(&command))
			.unwrap_or_else(|| String::from("shell"));

		if command_category == "pr_land" {
			return (CHILD_BUCKET_PR_LAND.to_owned(), String::from("exec_command: pr_land"));
		}

		return (CHILD_BUCKET_SHELL.to_owned(), format!("exec_command: {command_category}"));
	}

	(CHILD_BUCKET_TOOL.to_owned(), tool_name.to_owned())
}

fn is_tracker_tool_name(normalized_tool: &str) -> bool {
	matches!(
		normalized_tool,
		"issue_transition"
			| "issue_comment"
			| "issue_progress_checkpoint"
			| "issue_review_checkpoint"
			| "issue_review_handoff"
			| "issue_review_repair_complete"
			| "issue_delivery_closeout_complete"
			| "issue_terminal_finalize"
			| "issue_label_add"
	) || normalized_tool.ends_with(".issue_transition")
		|| normalized_tool.ends_with(".issue_comment")
		|| normalized_tool.ends_with(".issue_progress_checkpoint")
		|| normalized_tool.ends_with(".issue_review_checkpoint")
		|| normalized_tool.ends_with(".issue_review_handoff")
		|| normalized_tool.ends_with(".issue_review_repair_complete")
		|| normalized_tool.ends_with(".issue_delivery_closeout_complete")
		|| normalized_tool.ends_with(".issue_terminal_finalize")
		|| normalized_tool.ends_with(".issue_label_add")
}

fn shell_command_category(command: &str) -> String {
	let trimmed = command.trim();
	let lowered = trimmed.to_ascii_lowercase();

	if lowered.starts_with("git push")
		|| lowered.starts_with("gh pr")
		|| lowered.contains(" gh pr ")
		|| lowered.contains("decodex land")
		|| lowered.contains("issue_terminal_finalize")
	{
		return String::from("pr_land");
	}
	if lowered.starts_with("cargo make")
		|| lowered.starts_with("cargo test")
		|| lowered.starts_with("npm run check")
		|| lowered.contains(" nextest ")
	{
		return String::from("checks");
	}
	if lowered.starts_with("git ") {
		return String::from("git");
	}
	if lowered.starts_with("gh ") {
		return String::from("gh");
	}
	if lowered.contains("vite") || lowered.contains("dev server") || lowered.contains("localhost") {
		return String::from("dev_server");
	}
	if lowered.contains("playwright") || lowered.contains("browser") {
		return String::from("browser_smoke");
	}

	String::from("shell")
}

fn child_activity_bucket_mut<'a>(
	summary: &'a mut state::ChildAgentActivitySummary,
	name: &str,
) -> &'a mut state::ChildAgentActivityBucket {
	if let Some(index) = summary.buckets.iter().position(|bucket| bucket.name == name) {
		return &mut summary.buckets[index];
	}

	summary.buckets.push(state::ChildAgentActivityBucket {
		name: name.to_owned(),
		..state::ChildAgentActivityBucket::default()
	});

	let last_index = summary.buckets.len().saturating_sub(1);

	&mut summary.buckets[last_index]
}

fn extract_tool_name(value: &Value) -> Option<String> {
	let tool = string_at_paths(
		value,
		&[
			&["params", "tool"],
			&["params", "name"],
			&["params", "item", "tool"],
			&["params", "item", "name"],
			&["tool"],
			&["name"],
			&["item", "tool"],
			&["item", "name"],
		],
	)?;
	let namespace = string_at_paths(value, &[&["params", "namespace"], &["namespace"]]);

	Some(match namespace {
		Some(namespace) if !namespace.is_empty() => format!("{namespace}.{tool}"),
		_ => tool,
	})
}

fn extract_tool_arguments(value: &Value) -> Option<Value> {
	let arguments = value_at_paths(
		value,
		&[
			&["params", "arguments"],
			&["params", "item", "arguments"],
			&["arguments"],
			&["item", "arguments"],
		],
	)?;

	if let Some(arguments_text) = arguments.as_str()
		&& let Ok(parsed_arguments) = serde_json::from_str::<Value>(arguments_text)
	{
		return Some(parsed_arguments);
	}

	Some(arguments.clone())
}

fn extract_command_text(arguments: &Value) -> Option<String> {
	string_at_paths(arguments, &[&["cmd"], &["command"], &["argv", "0"]])
}

fn string_at_paths(value: &Value, paths: &[&[&str]]) -> Option<String> {
	paths
		.iter()
		.find_map(|path| value_at_path(value, path).and_then(Value::as_str).map(str::to_owned))
}

fn value_at_paths<'a>(value: &'a Value, paths: &[&[&str]]) -> Option<&'a Value> {
	paths.iter().find_map(|path| value_at_path(value, path))
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
	let mut current = value;

	for part in path {
		current = current.get(*part)?;
	}

	Some(current)
}

fn tool_output_size(value: Option<&Value>, payload: &str) -> i64 {
	let largest_string = value.map(largest_string_len).unwrap_or(0);
	let payload_len = i64::try_from(payload.len()).unwrap_or(i64::MAX);

	largest_string.max(payload_len)
}

fn largest_string_len(value: &Value) -> i64 {
	match value {
		Value::String(text) => i64::try_from(text.len()).unwrap_or(i64::MAX),
		Value::Array(items) => items.iter().map(largest_string_len).max().unwrap_or(0),
		Value::Object(entries) => entries.values().map(largest_string_len).max().unwrap_or(0),
		_ => 0,
	}
}

fn find_numeric_field(value: &Value, keys: &[&str]) -> Option<i64> {
	match value {
		Value::Object(entries) => {
			for (key, nested) in entries {
				if keys.iter().any(|candidate| *candidate == key)
					&& let Some(number) = json_number_to_i64(nested)
				{
					return Some(number);
				}
			}

			entries.values().find_map(|nested| find_numeric_field(nested, keys))
		},
		Value::Array(items) => items.iter().find_map(|nested| find_numeric_field(nested, keys)),
		_ => None,
	}
}

fn find_string_field(value: &Value, keys: &[&str]) -> Option<String> {
	match value {
		Value::Object(entries) => {
			for (key, nested) in entries {
				if keys.iter().any(|candidate| *candidate == key)
					&& let Some(text) = string_like_json_value(nested)
				{
					return Some(text);
				}
			}

			entries.values().find_map(|nested| find_string_field(nested, keys))
		},
		Value::Array(items) => items.iter().find_map(|nested| find_string_field(nested, keys)),
		_ => None,
	}
}

fn string_like_json_value(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		Value::Object(entries) => ["kind", "type"]
			.iter()
			.find_map(|key| entries.get(*key).and_then(string_like_json_value)),
		_ => None,
	}
}

fn json_number_to_i64(value: &Value) -> Option<i64> {
	value.as_i64().or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

pub(super) fn redact_identifier(identifier: &str) -> String {
	let tail =
		identifier.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}

fn duration_seconds_i64(duration: Duration) -> i64 {
	i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}
