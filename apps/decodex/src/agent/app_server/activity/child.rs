use std::{collections::HashMap, time::Instant};

use serde_json::{self, Value};
use time::OffsetDateTime;

use crate::state;

use super::{
	CHILD_BUCKET_BROWSER_IMAGE, CHILD_BUCKET_MODEL, CHILD_BUCKET_PR_LAND, CHILD_BUCKET_PROTOCOL,
	CHILD_BUCKET_SHELL, CHILD_BUCKET_TOOL, CHILD_BUCKET_TRACKER, INPUT_TOKEN_KEYS,
	LARGE_CHILD_OUTPUT_BYTES, OUTPUT_TOKEN_KEYS, duration_seconds_i64,
	payload::{
		extract_command_text, extract_tool_arguments, extract_tool_name, find_numeric_field,
		string_at_paths, tool_output_size,
	},
};

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

pub(in crate::agent::app_server) struct ChildActivityAccumulator {
	started_at: Instant,
	last_observed_at: Instant,
	current_bucket: Option<String>,
	current_detail: Option<String>,
	active_tool_name: Option<String>,
	large_output_stats: HashMap<String, LargeOutputStats>,
	summary: state::ChildAgentActivitySummary,
}
impl ChildActivityAccumulator {
	pub(in crate::agent::app_server) fn new() -> Self {
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

	pub(in crate::agent::app_server) fn record(
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
		"item/tool/call" =>
			child_tool_call_event(payload_value.as_ref(), input_tokens, output_tokens),
		"item/tool/call/response" =>
			child_tool_response_event(payload_value.as_ref(), active_tool_name, payload),
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
