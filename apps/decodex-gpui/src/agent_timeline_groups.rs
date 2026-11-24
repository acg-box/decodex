//! Present completed native turns without discarding their process records.
use std::collections::{BTreeMap, BTreeSet};

use gpui::{
	AnyElement, Context, KeyDownEvent, SharedString,
	prelude::{
		InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	shell::agent_surface::native_timeline::{
		AgentSurface, AgentTimelineContent, AgentTimelineEntry, AgentWorkItemDto, ItemKey,
		motion::ToolBatch,
	},
	ui_motion, ui_theme,
};

use super::super::conversation_divider::{self, Style};

pub(super) const HEADER_HEIGHT: f32 = Style::Process.height();

pub(super) fn process_turn(entry: &AgentTimelineEntry) -> Option<&str> {
	match &entry.content {
		AgentTimelineContent::Item { turn_id, kind, phase, attachments, activity, .. }
			if attachments.is_empty()
				&& (matches!(
					kind.as_str(),
					"reasoning" | "plan" | "functionCallOutput" | "sleep"
				) || (kind == "agentMessage" && phase.as_deref() == Some("commentary"))
					|| (!matches!(
						kind.as_str(),
						"agentMessage" | "userMessage" | "agentInput"
					) && activity.is_some())) =>
			Some(turn_id),
		_ => None,
	}
}

pub(super) fn row_gap(previous: &AgentTimelineEntry, current: &AgentTimelineEntry) -> f32 {
	let turn = process_turn(current);
	if turn.is_some() && process_turn(previous) == turn {
		ui_theme::PROCESS_STEP_GAP
	} else {
		ui_theme::MESSAGE_GAP
	}
}

pub(super) struct Group {
	/// Exact prompt that owns this folded response, when present in this page.
	pub input_index: Option<usize>,
	pub turn: String,
	pub turns: BTreeSet<String>,
	pub indices: Vec<usize>,
	pub expanded: bool,
	pub first_index: usize,
	/// The preserved final answer or terminal boundary.
	pub final_index: usize,
	pub count: usize,
	pub observations: Vec<decodex_protocol::AgentActivityDto>,
}

pub(super) fn with_observations(
	entries: &[AgentTimelineEntry],
	expanded: &BTreeSet<String>,
	observations: &[decodex_protocol::AgentActivityDto],
) -> Vec<Group> {
	responses_with_observations(entries, expanded, observations)
		.into_iter()
		.filter(|group| group.count > 0)
		.collect()
}

pub(super) fn responses_with_observations(
	entries: &[AgentTimelineEntry],
	expanded: &BTreeSet<String>,
	observations: &[decodex_protocol::AgentActivityDto],
) -> Vec<Group> {
	let mut result = responses(entries, expanded);
	for activity in observations {
		if entries.iter().any(|entry| {
			matches!(&entry.content,
			AgentTimelineContent::Item { turn_id, item_id, .. }
			if turn_id == &activity.turn_id && item_id == &activity.item_id)
		}) {
			continue;
		}
		let Some(terminal) = entries.iter().position(|entry| {
			matches!(&entry.content,
			AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. }
			if turn_id == &activity.turn_id)
		}) else {
			continue;
		};
		let index = if let Some(index) =
			result.iter().position(|group| group.turns.contains(&activity.turn_id))
		{
			index
		} else {
			let final_index = entries[..terminal].iter().rposition(|entry| matches!(&entry.content,
				AgentTimelineContent::Item { turn_id, kind, phase, .. }
				if turn_id == &activity.turn_id && kind == "agentMessage" && phase.as_deref() != Some("commentary")))
				.unwrap_or(terminal);
			result.push(Group {
				input_index: opening_inputs(entries).into_iter().find(|&index| matches!(
					&entries[index].content, AgentTimelineContent::Item { turn_id, .. } if turn_id == &activity.turn_id
				)),
				turn: activity.turn_id.clone(),
				turns: BTreeSet::from([activity.turn_id.clone()]),
				indices: vec![],
				expanded: expanded.contains(&activity.turn_id),
				first_index: final_index,
				final_index,
				count: 0,
				observations: vec![],
			});
			result.len() - 1
		};
		result[index].observations.push(activity.clone());
		result[index].count += 1;
	}
	result.sort_by_key(|group| group.first_index);
	result
}

impl AgentSurface {
	pub(super) fn turn_process_header(
		&self,
		work: &AgentWorkItemDto,
		group: &Group,
		entry: &AgentTimelineEntry,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let turn = group.turn.clone();
		let turns = group.turns.clone();
		let expanded = group.expanded;
		self.process_header(
			work,
			entry,
			process_label(group.count),
			expanded,
			move |s| {
				s.timeline.native.expanded_turns.retain(|id| !turns.contains(id));
				if !expanded {
					s.timeline.native.expanded_turns.insert(turn.clone());
				}
			},
			cx,
		)
	}

	pub(super) fn tool_batch_body(
		&self,
		work: &AgentWorkItemDto,
		start: usize,
		batch: ToolBatch,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let entry = &self.timeline.native.entries[start];
		let key = ItemKey::for_entry(entry).expect("tool batch anchor");
		let header = self.process_header(
			work,
			entry,
			tool_batch_label(batch.count),
			batch.expanded,
			move |s| s.timeline.native.arrivals.expand_tool_batch(&key, !batch.expanded),
			cx,
		);
		let identity = process_identity(work, entry);
		let owner = cx.entity();
		let work = work.clone();

		gpui::div()
			.w_full()
			.debug_selector(|| "tool-batch-block".into())
			.child(header)
			.child(ui_motion::disclosure_lazy(
				SharedString::from(format!("tool-batch-body-{identity}")),
				batch.expanded,
				move |cx| owner.update(cx, |s, cx| s.tool_batch_details(&work, start, batch, cx)),
			))
			.into_any_element()
	}

	pub(super) fn outgoing_tool_batch(
		&self,
		work: &AgentWorkItemDto,
		start: usize,
		batch: ToolBatch,
		keeps_header: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let entry = &self.timeline.native.entries[start];
		let header = (!keeps_header).then(|| {
			let caption = conversation_divider::caption(
				&process_identity(work, entry),
				tool_batch_label(batch.count),
				batch.expanded,
				Style::Process,
			);
			conversation_divider::row(caption.into_any_element(), Style::Process)
		});
		gpui::div()
			.w_full()
			.children(header)
			.children(batch.expanded.then(|| self.tool_batch_details(work, start, batch, cx)))
			.into_any_element()
	}

	fn tool_batch_details(
		&self,
		work: &AgentWorkItemDto,
		start: usize,
		batch: ToolBatch,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let indices = (start..start + batch.count).collect::<Vec<_>>();
		let entry = &self.timeline.native.entries[start];
		gpui::div()
			.pt(gpui::px(ui_theme::PROCESS_STEP_GAP))
			.child(self.process_history_content(work, &indices, &[], entry, None, cx))
			.into_any_element()
	}

	fn process_header(
		&self,
		work: &AgentWorkItemDto,
		entry: &AgentTimelineEntry,
		label: String,
		expanded: bool,
		toggle: impl Fn(&mut Self) + Clone + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let identity = process_identity(work, entry);
		let action = (work.id.clone(), entry.clone());
		let keyboard = action.clone();
		let toggle_key = toggle.clone();
		let control = conversation_divider::control(identity, label, expanded, Style::Process)
			.debug_selector(|| "turn-process-toggle".into())
			.on_click(cx.listener(move |s, _, _, cx| {
				s.anchor_process_toggle(&action.0, &action.1);
				toggle(s);
				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) && !event.is_held {
					s.anchor_process_toggle(&keyboard.0, &keyboard.1);
					toggle_key(s);
					cx.notify();
					cx.stop_propagation();
				}
			}))
			.into_any_element();
		conversation_divider::row(control, Style::Process).into_any_element()
	}
}

fn process_identity(work: &AgentWorkItemDto, entry: &AgentTimelineEntry) -> String {
	let identity = serde_json::json!([work.id, work.codex_thread_id, super::key(entry)]);
	format!("turn-process-{identity}")
}

fn tool_batch_label(count: usize) -> String {
	format!("Tool calls ({count})")
}

/// Empty finished messages have no content to render. Process records stay visible.
pub(super) fn hidden_items(entries: &[AgentTimelineEntry]) -> BTreeSet<usize> {
	let completed: BTreeSet<_> = entries
		.iter()
		.filter_map(|entry| match &entry.content {
			AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } =>
				Some(turn_id.as_str()),
			_ => None,
		})
		.collect();

	entries
		.iter()
		.enumerate()
		.filter_map(|(index, entry)| match &entry.content {
			AgentTimelineContent::Item {
				turn_id,
				kind,
				text,
				truncated: false,

				attachments,
				..
			} if matches!(kind.as_str(), "reasoning" | "agentMessage")
				&& text.trim().is_empty()
				&& attachments.is_empty()
				&& completed.contains(turn_id.as_str()) =>
				Some(index),
			_ => None,
		})
		.collect()
}

/// A user response can span several native turns (worker wakes have no user input).
/// Keep its last completed answer and fold the preceding execution records together.
#[cfg(test)]
pub(super) fn groups(entries: &[AgentTimelineEntry], expanded: &BTreeSet<String>) -> Vec<Group> {
	responses(entries, expanded).into_iter().filter(|group| group.count > 0).collect()
}

/// Only the opening input of a native turn starts a response. A page can start
/// in the middle of a turn: a user item after observed work is then a steer.
fn opening_inputs(entries: &[AgentTimelineEntry]) -> BTreeSet<usize> {
	let mut seen = BTreeSet::new();
	let mut started = BTreeSet::new();
	let mut inputs = BTreeSet::new();
	for (index, entry) in entries.iter().enumerate() {
		match &entry.content {
			AgentTimelineContent::TurnBoundary { turn_id, completed: false, .. } => {
				started.insert(turn_id);
			},
			AgentTimelineContent::Item { turn_id, kind, .. } => {
				let first_item = seen.insert(turn_id);
				if matches!(kind.as_str(), "userMessage" | "agentInput") {
					let has_start = started.remove(turn_id);
					if first_item || has_start {
						inputs.insert(index);
					}
				}
			},
			_ => {},
		}
	}
	inputs
}

fn responses(entries: &[AgentTimelineEntry], expanded: &BTreeSet<String>) -> Vec<Group> {
	let empty = hidden_items(entries);
	let inputs = opening_inputs(entries);
	let mut owners = BTreeMap::new();
	for &index in &inputs {
		if let AgentTimelineContent::Item { turn_id, .. } = &entries[index].content {
			owners.insert(turn_id.as_str(), index);
		}
	}
	let terminal: BTreeMap<_, _> = entries
		.iter()
		.enumerate()
		.filter_map(|(index, entry)| {
			let AgentTimelineContent::TurnBoundary {
				turn_id, completed: true, status, error, ..
			} = &entry.content
			else {
				return None;
			};
			match status.as_deref() {
				Some("completed") if error.is_none() => Some((turn_id.as_str(), (index, false))),
				Some("failed" | "interrupted") => Some((turn_id.as_str(), (index, true))),
				_ => None,
			}
		})
		.collect();
	let mut responses = BTreeMap::<(usize, Option<usize>), (usize, Vec<usize>)>::new();
	let mut range = 0;
	for (index, entry) in entries.iter().enumerate() {
		match &entry.content {
			AgentTimelineContent::Item { turn_id, .. } => {
				if inputs.contains(&index) {
					range = index;
				}
				if let Some(&(end, failed)) = terminal.get(turn_id.as_str()) {
					// Late items follow their native turn. An input-free worker wake
					// continues the response in which its work began.
					let owner = owners.entry(turn_id.as_str()).or_insert(range);
					let (terminal, items) =
						responses.entry((*owner, failed.then_some(end))).or_default();
					*terminal = (*terminal).max(end);
					items.push(index);
				}
			},
			AgentTimelineContent::Speech { .. }
			| AgentTimelineContent::VoiceBoundary { .. }
			| AgentTimelineContent::Promotion { .. } => range = index,
			_ => {},
		}
	}
	let mut result: Vec<_> = responses
		.into_iter()
		.map(|((range, failure), (end, items))| {
			response_group(
				entries,
				inputs.get(&range).copied(),
				&items,
				(end, failure.is_some()),
				&empty,
				expanded,
			)
		})
		.collect();
	result.sort_by_key(|group| group.first_index);
	result
}

fn response_group(
	entries: &[AgentTimelineEntry],
	input: Option<usize>,
	items: &[usize],
	(end, failed): (usize, bool),
	empty: &BTreeSet<usize>,
	expanded: &BTreeSet<String>,
) -> Group {
	let answer = items.iter().copied().rev().find(|i| {
		matches!(&entries[*i].content, AgentTimelineContent::Item {
			kind, phase, text, attachments, truncated, ..
		} if kind == "agentMessage" && matches!(phase.as_deref(), None | Some("final_answer"))
			&& (!text.trim().is_empty() || !attachments.is_empty() || *truncated))
	});
	let final_index = if failed { end } else { answer.unwrap_or(end) };
	let indices: Vec<_> = items
		.iter()
		.copied()
		.filter(|i| Some(*i) != input && Some(*i) != answer && !empty.contains(i))
		.collect();
	let first_index = indices
		.iter()
		.copied()
		.find(|i| input.is_none_or(|prompt| *i > prompt))
		.unwrap_or(final_index);
	let turns: BTreeSet<_> = items
		.iter()
		.filter_map(|i| match &entries[*i].content {
			AgentTimelineContent::Item { turn_id, .. } => Some(turn_id.clone()),
			_ => None,
		})
		.collect();
	let anchor = input.filter(|i| items.contains(i)).unwrap_or(items[0]);
	let AgentTimelineContent::Item { turn_id, .. } = &entries[anchor].content else {
		unreachable!("native item")
	};
	Group {
		input_index: input,
		turn: turn_id.clone(),
		expanded: !expanded.is_disjoint(&turns),
		turns,
		first_index: first_index.min(final_index),
		final_index,
		count: indices.len(),
		observations: Vec::new(),
		indices,
	}
}
fn process_label(count: usize) -> String {
	crate::shell::agent_surface::conversation_metric::steps_label(count)
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(crate) fn visual_process_page(&mut self, cx: &mut Context<Self>) {
		use decodex_protocol::{AgentActivityDetailResult, AgentTimelinePage};
		self.visual_workspace_fixture(cx);
		self.workspace.graph_visible = false;
		self.workspace.sidebar_visible = false;
		self.workspace.agent_tree_visible = false;
		let work = self.selected.clone().expect("process fixture selection");
		let snapshot = self.snapshot.as_mut().expect("process fixture snapshot");
		snapshot.runtime_source = Some(
			decodex_protocol::EntityId::new("process-preview").expect("process fixture source"),
		);
		snapshot.work_items.retain(|item| item.id == work);
		snapshot.dependencies.clear();
		snapshot.pending_events.clear();
		snapshot.work_items[0].codex_thread_id = Some("process-preview".into());
		let entries = process_preview_entries();
		self.timeline.native.replace(
			super::Binding {
				work: work.clone(),
				thread: "process-preview".into(),
				account: "preview".into(),
			},
			AgentTimelinePage {
				thread_id: "process-preview".into(),
				entries,
				next_cursor: None,
				source_threads: Default::default(),
				weather: Default::default(),
				observed_activities: vec![],
				thread_token_total: None,
				thread_token_breakdown: None,
				thread_metrics: None,
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None,
			},
		);
		self.timeline.native.expanded_turns.insert("preview-turn".into());
		if let Some(key) = self.activity_detail_key(&(work, "preview-turn".into(), "test".into())) {
			self.activity_detail.value = Some((
				key,
				Some(AgentActivityDetailResult::Available {
					text: "cargo test -p exporter\n\n8 passed; 0 failed\n\nExit code: 0".into(),
					truncated: false,
					offset: 0,
					next: None,
				}),
			));
		}
	}
}

#[cfg(any(test, feature = "visual-capture"))]
fn process_preview_entries() -> Vec<AgentTimelineEntry> {
	use decodex_protocol::AgentActivityDto;
	let item = |position, id: &str, kind: &str, text: &str, phase: Option<&str>, activity| {
		AgentTimelineEntry {
			position,
			content: AgentTimelineContent::Item {
				turn_id: "preview-turn".into(),
				item_id: id.into(),
				kind: kind.into(),
				phase: phase.map(str::to_owned),
				text: text.into(),
				truncated: false,
				activity,
				attachments: vec![],
				collaboration: None,
			},
		}
	};
	let mut entries = vec![
		item(
			0,
			"prompt",
			"userMessage",
			"Check the export changes and verify the result.",
			None,
			None,
		),
		item(
			1,
			"reasoning",
			"reasoning",
			"### Verify the change\n\nCheck the **source, tests and output** before handing it back.",
			None,
			None,
		),
	];
	for (index, (id, kind, status, label, duration, lines)) in [
		("read", "commandExecution", "completed", "File read", Some(12), Some(24)),
		("test", "commandExecution", "completed", "Tests passed", Some(1380), Some(8)),
		("remote", "mcpToolCall", "failed", "Reference source unavailable", Some(620), None),
		("declined", "commandExecution", "declined", "Optional check declined", None, None),
		("stopped", "commandExecution", "interrupted", "Background check stopped", None, None),
		("wait", "sleep", "recorded", "Wait", Some(1200), None),
		("unknown", "commandExecution", "unreported", "Result not reported", None, None),
		("running", "commandExecution", "running", "Background check", None, None),
	]
	.into_iter()
	.enumerate()
	{
		let activity = AgentActivityDto {
			turn_id: "preview-turn".into(),
			item_id: id.into(),
			kind: kind.into(),
			status: status.into(),
			label: label.into(),
			detail: String::new(),
			duration_ms: duration,
			retained_output_lines: lines,
			review: None,
			plugin_id: None,
			read_only_hint: None,
			native_timestamp_ms: None,
		};
		entries.push(item(index as u64 + 2, id, kind, "", None, Some(activity)));
	}
	entries.push(item(
		10,
		"answer",
		"agentMessage",
		"The source and tests are verified. The optional reference could not be reached.",
		Some("final_answer"),
		None,
	));
	entries.push(AgentTimelineEntry {
		position: 11,
		content: AgentTimelineContent::TurnBoundary {
			started_at_micros: None,
			turn_id: "preview-turn".into(),
			completed: true,
			status: Some("completed".into()),
			duration_ms: Some(3240),
			usage_summary: None,
			usage: Some(decodex_protocol::AgentTurnUsageDto {
				input_tokens: 9750,
				output_tokens: 418,
				details: None,
			}),
			timing: None,
			error: None,
		},
	});
	entries
}

#[cfg(test)]
mod tests {
	use std::thread;

	use gpui::AppContext as _;

	use crate::shell::agent_surface::native_timeline::{
		Binding, Timeline,
		groups::{self, AgentSurface, AgentTimelineContent, AgentTimelineEntry, BTreeSet},
		motion::ToolBatch,
	};

	#[test]
	fn a_silent_completed_turn_keeps_observed_steps_with_its_worker_reply() {
		let entries = vec![
			message(0, "userMessage", None),
			completed("completed"),
			in_turn(message(2, "agentMessage", Some("final_answer")), "wake"),
			in_turn(completed("completed"), "wake"),
		];
		let observed = serde_json::from_value(serde_json::json!({
			"turn_id":"turn", "item_id":"observed-only", "kind":"commandExecution",
			"status":"unreported", "label":"Read source", "detail":"", "duration_ms":null
		}))
		.unwrap();
		let result = groups::responses_with_observations(&entries, &BTreeSet::new(), &[observed]);
		assert_eq!(result.len(), 1);
		assert_eq!(result[0].input_index, Some(0));
		assert_eq!(result[0].final_index, 2);
		assert_eq!(result[0].count, 1, "the observed step must not become a false zero");
	}

	#[test]
	fn observed_commands_stay_in_the_finished_process_until_native_history_contains_them() {
		let entries =
			vec![message(1, "agentMessage", Some("final_answer")), completed("completed")];
		let command: decodex_protocol::AgentActivityDto =
			serde_json::from_value(serde_json::json!({
				"turn_id":"turn","item_id":"command","kind":"commandExecution","status":"unreported",
				"label":"Reading files","detail":"Result not reported","duration_ms":null
			}))
			.unwrap();
		let groups =
			groups::with_observations(&entries, &BTreeSet::new(), std::slice::from_ref(&command));
		assert_eq!(groups.len(), 1);
		assert_eq!(groups[0].count, 1);
		assert_eq!(groups[0].first_index, 0);
		assert!(!groups[0].expanded);
		assert_eq!(groups[0].observations[0].status, "unreported");
		let mut native = message(0, "commandExecution", None);
		if let AgentTimelineContent::Item { item_id, .. } = &mut native.content {
			*item_id = "command".into();
		}
		let recovered = groups::with_observations(
			&[vec![native], entries].concat(),
			&BTreeSet::new(),
			&[command],
		);
		assert!(recovered.iter().all(|group| group.observations.is_empty()));
	}

	#[test]
	fn a_late_tool_result_stays_with_its_original_reply() {
		let entries = vec![
			message(0, "userMessage", None),
			message(1, "agentMessage", Some("final_answer")),
			completed("completed"),
			in_turn(message(3, "userMessage", None), "later"),
			in_turn(message(4, "reasoning", None), "later"),
			in_turn(message(5, "agentMessage", Some("final_answer")), "later"),
			in_turn(completed("completed"), "later"),
			message(7, "functionCallOutput", None),
		];
		let result = groups::groups(&entries, &BTreeSet::new());
		assert_eq!(result.len(), 2);
		assert_eq!(result[0].turn, "turn");
		assert_eq!(result[0].indices, [7]);
		assert_eq!(result[0].first_index, 1);
		assert_eq!(result[1].turn, "later");
		assert_eq!(result[1].indices, [4]);
	}

	fn message(index: u64, kind: &str, phase: Option<&str>) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: index,
			content: AgentTimelineContent::Item {
				collaboration: None,
				turn_id: "turn".into(),
				item_id: index.to_string(),
				kind: kind.into(),
				phase: phase.map(str::to_owned),
				text: "Public content".into(),
				truncated: false,
				activity: None,

				attachments: vec![],
			},
		}
	}
	fn completed(status: &str) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: 4,
			content: AgentTimelineContent::TurnBoundary {
				started_at_micros: None,
				turn_id: "turn".into(),
				completed: true,
				status: Some(status.into()),
				duration_ms: Some(500),
				timing: None,
				usage_summary: None,
				usage: None,
				error: None,
			},
		}
	}
	#[test]
	fn empty_reasoning_disappears_only_after_completion_and_does_not_count_as_process() {
		let mut entries =
			vec![message(1, "reasoning", None), message(2, "agentMessage", Some("final_answer"))];

		if let AgentTimelineContent::Item { text, .. } = &mut entries[0].content {
			*text = " \n\t".into();
		}

		assert!(groups::hidden_items(&entries).is_empty());

		entries.push(completed("completed"));

		assert_eq!(groups::hidden_items(&entries), BTreeSet::from([0]));
		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());

		if let AgentTimelineContent::Item { truncated, .. } = &mut entries[0].content {
			*truncated = true;
		}

		assert!(groups::hidden_items(&entries).is_empty());

		if let AgentTimelineContent::Item { text, truncated, .. } = &mut entries[0].content {
			*text = "Retained summary".into();
			*truncated = false;
		}

		assert!(groups::hidden_items(&entries).is_empty());
		assert_eq!(groups::groups(&entries, &BTreeSet::new())[0].count, 1);
	}

	#[test]
	fn waiting_for_a_question_retains_each_wait_in_the_completed_process() {
		let mut entries = vec![
			message(0, "userMessage", None),
			message(1, "agentMessage", Some("final_answer")),
			message(2, "sleep", None),
			message(3, "sleep", None),
			message(4, "sleep", None),
			message(5, "userMessage", None),
			message(6, "agentMessage", Some("final_answer")),
		];

		for entry in &mut entries[2..5] {
			if let AgentTimelineContent::Item { text, .. } = &mut entry.content {
				text.clear();
			}
		}

		assert!(groups::hidden_items(&entries).is_empty());

		entries.push(completed("completed"));

		assert!(groups::hidden_items(&entries).is_empty());

		let groups = groups::groups(&entries, &BTreeSet::new());

		assert_eq!(groups.len(), 1);
		assert_eq!(groups[0].indices, [1, 2, 3, 4, 5]);
		assert_eq!(groups[0].final_index, 6);

		if let AgentTimelineContent::Item { text, .. } = &mut entries[2].content {
			*text = "Could not wait for input".into();
		}

		assert!(groups::hidden_items(&entries).is_empty());
	}

	#[test]
	fn terminal_turns_fold_process_without_hiding_final_answers() {
		let mut entries = vec![
			message(0, "userMessage", None),
			message(1, "agentMessage", Some("commentary")),
			message(2, "reasoning", None),
			message(3, "agentMessage", Some("final_answer")),
		];

		assert!(
			groups::groups(&entries, &BTreeSet::new()).is_empty(),
			"final output can arrive before work finishes"
		);

		entries.push(completed("completed"));

		let result = groups::groups(&entries, &BTreeSet::new());

		assert_eq!(result[0].indices, vec![1, 2]);
		assert!(!result[0].expanded);
		assert!(groups::groups(&entries, &BTreeSet::from(["turn".into()]))[0].expanded);

		entries[4] = completed("failed");

		let failed = groups::groups(&entries, &BTreeSet::new());
		assert_eq!(failed[0].indices, [1, 2]);
		assert_eq!(failed[0].final_index, 4);
		assert!(!failed[0].indices.contains(&3), "keep the final answer visible");
		assert!(!failed[0].indices.contains(&4), "keep the failure visible");

		entries[4] = completed("interrupted");

		assert_eq!(groups::groups(&entries, &BTreeSet::new())[0].indices, [1, 2]);
	}
	#[test]
	fn failed_process_without_a_final_answer_can_be_reopened() {
		let entries = vec![
			message(0, "userMessage", None),
			message(1, "reasoning", None),
			message(2, "agentMessage", Some("commentary")),
			completed("failed"),
		];
		let folded = groups::groups(&entries, &BTreeSet::new());
		assert_eq!(folded[0].indices, [1, 2]);
		assert_eq!(folded[0].final_index, 3);
		assert!(!folded[0].expanded);
		assert!(groups::groups(&entries, &BTreeSet::from(["turn".into()]))[0].expanded);
	}

	#[test]
	fn completed_legacy_reply_without_phase_still_folds_its_process() {
		let mut entries = vec![
			message(0, "userMessage", None),
			message(1, "reasoning", None),
			message(2, "agentMessage", None),
		];
		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());
		entries.push(completed("completed"));
		let result = groups::groups(&entries, &BTreeSet::new());
		assert_eq!(result[0].indices, [1]);
		assert_eq!(result[0].final_index, 2);
	}

	#[gpui::test]
	fn live_process_rows_are_compact_and_keep_the_reply_separate(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));
		let entries = vec![
			message(0, "userMessage", None),
			message(1, "reasoning", None),
			message(2, "reasoning", None),
			message(3, "agentMessage", Some("final_answer")),
		];
		let selectors = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.graph_visible = false;
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();
			work.codex_thread_id = Some("native-thread".into());
			let selectors: Vec<_> = entries
				.iter()
				.map(|entry| {
					format!(
						"native-history-{}",
						serde_json::json!([
							work.id,
							work.codex_thread_id,
							super::super::key(entry)
						])
					)
				})
				.collect();
			s.timeline.native.replace(
				Binding {
					work: work.id.clone(),
					thread: "native-thread".into(),
					account: "account".into(),
				},
				decodex_protocol::AgentTimelinePage {
					source_threads: Default::default(),
					thread_id: "native-thread".into(),
					entries,
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_token_breakdown: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					observed_activities: Vec::new(),
					active_realtime_session_at_page_start: None,
				},
			);
			cx.notify();
			selectors
		});
		visual.update(crate::ui_frame::draw_frame);
		thread::sleep(std::time::Duration::from_millis(350));
		visual.update(crate::ui_frame::draw_frame);
		let first = visual
			.debug_bounds(Box::leak(selectors[1].clone().into_boxed_str()))
			.expect("first reasoning");
		let second = visual
			.debug_bounds(Box::leak(selectors[2].clone().into_boxed_str()))
			.expect("second reasoning");
		let reply =
			visual.debug_bounds(Box::leak(selectors[3].clone().into_boxed_str())).expect("reply");
		assert!(
			(second.top() - first.bottom() - gpui::px(crate::ui_theme::PROCESS_STEP_GAP)).abs()
				<= gpui::px(1.)
		);
		assert!(
			(reply.top() - second.bottom() - gpui::px(crate::ui_theme::MESSAGE_GAP)).abs()
				<= gpui::px(1.)
		);
	}
	#[test]
	fn an_unknown_phase_never_replaces_the_native_final_answer() {
		let mut entries = vec![
			message(0, "agentMessage", Some("future_phase")),
			message(1, "reasoning", None),
			message(2, "agentMessage", Some("future_phase")),
			completed("completed"),
		];

		let without_answer = groups::groups(&entries, &BTreeSet::new());
		assert_eq!(without_answer[0].indices, [0, 1, 2]);
		assert_eq!(
			without_answer[0].final_index, 3,
			"a completed turn can end without a final answer"
		);

		entries[2] = message(2, "agentMessage", Some("final_answer"));

		assert_eq!(groups::groups(&entries, &BTreeSet::new())[0].indices, vec![0, 1]);
	}
	#[test]
	fn one_native_turn_keeps_steers_live_and_folds_all_intermediate_items_on_completion() {
		// The captured turn had commentary and fifteen reasoning items before
		// a user steer, then eight reasoning items and one final answer.
		let mut entries = vec![message(0, "userMessage", None)];
		for i in 1..=15 {
			entries.push(message(i, "reasoning", None));
		}
		entries.push(message(16, "agentMessage", Some("commentary")));
		entries.push(message(17, "functionCallOutput", None));
		entries.push(steer_with_attachment(18));
		for i in 19..27 {
			entries.push(message(i, "reasoning", None));
		}
		entries.push(message(27, "agentInput", None));
		entries.push(message(28, "agentMessage", Some("commentary")));
		entries.push(message(29, "agentMessage", Some("final_answer")));
		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());
		let original = entries.clone();
		for status in ["completed", "interrupted", "failed"] {
			entries.push(completed(status));
			let groups = groups::groups(&entries, &BTreeSet::new());
			assert_eq!(groups.len(), 1);
			let group = &groups[0];
			assert_eq!(group.input_index, Some(0));
			assert_eq!(group.indices, (1..29).collect::<Vec<_>>());
			assert_eq!(group.count, 28);
			assert!(!group.expanded);
			assert_eq!(
				entries[group.indices[17]], original[18],
				"preserve the exact steer and attachment"
			);
			assert!(groups::groups(&entries, &BTreeSet::from(["turn".into()]))[0].expanded);
			let page = groups::groups(&entries[15..], &BTreeSet::from(["turn".into()]));
			assert!(page[0].expanded && page[0].input_index.is_none());
			assert!(
				page[0].indices.contains(&(18 - 15)),
				"a steer on a partial page stays inside its turn"
			);
			entries.pop();
		}
	}

	fn steer_with_attachment(index: u64) -> AgentTimelineEntry {
		let mut entry = message(index, "userMessage", None);
		if let AgentTimelineContent::Item { text, attachments, .. } = &mut entry.content {
			*text = "Please keep this exact line.".into();
			attachments.push(decodex_protocol::AgentTimelineAttachment {
				index: 0,
				kind: "image".into(),
				label: "example.png".into(),
				source: decodex_protocol::AgentTimelineAttachmentSource::Stored,
			});
		}
		entry
	}

	#[test]
	fn original_prompts_stay_in_separate_turns_with_late_tool_results_and_steers() {
		let entries = [
			message(0, "userMessage", None),
			message(1, "userMessage", None),
			message(2, "agentMessage", Some("final_answer")),
			completed("completed"),
			in_turn(message(4, "userMessage", None), "next"),
			in_turn(message(5, "userMessage", None), "next"),
			message(6, "functionCallOutput", None),
			in_turn(message(7, "agentMessage", Some("final_answer")), "next"),
			in_turn(completed("completed"), "next"),
		];
		assert_eq!(groups::opening_inputs(&entries), [0, 4].into());
		let groups = groups::groups(&entries, &BTreeSet::from(["next".into()]));
		assert_eq!(groups.len(), 2);
		assert_eq!((groups[0].input_index, groups[0].final_index), (Some(0), 2));
		assert_eq!((groups[1].input_index, groups[1].final_index), (Some(4), 7));
		assert_eq!(groups[0].indices, [1, 6]);
		assert_eq!(groups[1].indices, [5]);
		assert!(!groups[0].expanded && groups[1].expanded);
	}

	fn in_turn(mut entry: AgentTimelineEntry, turn: &str) -> AgentTimelineEntry {
		match &mut entry.content {
			AgentTimelineContent::Item { turn_id, .. }
			| AgentTimelineContent::TurnBoundary { turn_id, .. } => *turn_id = turn.into(),
			_ => unreachable!(),
		}

		entry
	}

	#[test]
	fn worker_wakes_and_superseded_finals_share_one_response_process() {
		let mut silent = in_turn(message(6, "agentMessage", Some("final_answer")), "wake");

		if let AgentTimelineContent::Item { text, .. } = &mut silent.content {
			text.clear();
		}

		let entries = vec![
			message(0, "userMessage", None),
			message(1, "agentMessage", Some("commentary")),
			message(2, "agentMessage", Some("final_answer")),
			completed("completed"),
			in_turn(message(5, "functionCallOutput", None), "wake"),
			silent,
			in_turn(completed("completed"), "wake"),
			in_turn(message(8, "functionCallOutput", None), "result"),
			in_turn(message(9, "agentMessage", Some("final_answer")), "result"),
			in_turn(completed("completed"), "result"),
		];
		let groups = groups::groups(&entries, &BTreeSet::new());

		assert_eq!(groups.len(), 1);
		assert_eq!(groups[0].indices, vec![1, 2, 4, 7]);
		assert_eq!(groups[0].first_index, 1);
		assert_eq!(groups[0].count, 4);
		assert_eq!(groups::hidden_items(&entries), BTreeSet::from([5]));
		assert!(groups::groups(&entries, &BTreeSet::from(["wake".into()]))[0].expanded);
	}

	#[test]
	fn silent_wake_process_sits_before_the_last_visible_answer() {
		let entries = vec![
			message(0, "userMessage", None),
			message(1, "agentMessage", Some("final_answer")),
			completed("completed"),
			in_turn(message(3, "functionCallOutput", None), "wake"),
			in_turn(completed("completed"), "wake"),
		];
		let groups = groups::groups(&entries, &BTreeSet::new());

		assert_eq!(groups[0].first_index, 1);
		assert_eq!(groups[0].indices, vec![3]);
	}

	#[test]
	fn active_wakes_and_failed_turns_are_not_folded_into_a_completed_reply() {
		let entries = vec![
			message(0, "userMessage", None),
			message(1, "agentMessage", Some("final_answer")),
			completed("completed"),
			in_turn(message(3, "functionCallOutput", None), "wake"),
			in_turn(message(4, "agentMessage", Some("final_answer")), "wake"),
			in_turn(completed("failed"), "wake"),
		];

		let result = groups::groups(&entries, &BTreeSet::new());
		assert_eq!(result.len(), 1);
		assert_eq!(result[0].turn, "wake");
		assert_eq!(
			result[0].indices,
			[3],
			"failed work has its own process, not the completed reply"
		);
		assert!(groups::groups(&entries[..5], &BTreeSet::new()).is_empty());
	}

	#[gpui::test]
	fn a_new_turn_with_reused_item_ids_keeps_old_processes_folded(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, _| {
			let entries = vec![
				message(1, "reasoning", None),
				message(2, "agentMessage", Some("final_answer")),
				completed("completed"),
			];
			let groups = groups::groups(&entries, &BTreeSet::new());
			let state = &s.timeline.native.completions;
			state.prepare(&entries, &groups, &Default::default(), |_| None);
			state.output("turn", "2").set(true);
			state.present_live("next-turn", "1");
			assert!(!state.output("next-turn", "2").get());
			state.prepare(&entries, &groups, &Default::default(), |_| None);
			assert!(state.get(&groups[0]).is_none(), "old steps must not start closing again");
			assert!(state.output("turn", "2").get(), "old answer completion stays independent");
		});
	}

	#[gpui::test]
	fn live_only_process_is_folded_once_when_the_first_saved_page_is_complete(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			let entries = vec![
				message(1, "reasoning", None),
				message(2, "agentMessage", Some("final_answer")),
				completed("completed"),
			];
			let groups = groups::groups(&entries, &BTreeSet::new());
			let state = &mut s.timeline.native.completions;

			state.present_live("turn", "1");
			state.replace_history(true);
			state.prepare(&entries, &groups, &Default::default(), |_| None);

			let motion = state.get(&groups[0]).unwrap();

			assert!(motion.outgoing.keys().copied().eq([1]));

			state.replace_history(true);
			state.prepare(&entries, &groups, &Default::default(), |_| None);

			assert!(std::rc::Rc::ptr_eq(&motion, &state.get(&groups[0]).unwrap()));

			state.present_live("turn", "1");
			state.replace_history(false);
			state.prepare(&entries, &groups, &Default::default(), |_| None);

			assert!(state.get(&groups[0]).is_none(), "a different source is static history");

			state.prepare(&entries[..2], &[], &Default::default(), |_| None);
			state.replace_history(true);
			state.prepare(&entries, &groups, &Default::default(), |_| None);

			assert!(
				state.get(&groups[0]).is_some(),
				"a native page replacement must retain displayed rows too"
			);
		});
	}

	#[gpui::test]
	fn a_tool_batch_folds_from_the_details_the_reader_actually_saw(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, _| {
			let mut entries = vec![message(0, "userMessage", None)];
			entries.extend((1..=4).map(|i| message(i, "functionCallOutput", None)));
			entries.push(message(5, "agentMessage", Some("final_answer")));
			entries.push(AgentTimelineEntry { position: 6, ..completed("completed") });
			let groups = groups::groups(&entries, &BTreeSet::new());
			let state = &mut s.timeline.native.completions;
			for expanded in [false, true] {
				let batches = [(1, ToolBatch { count: 4, expanded })].into();
				state.replace_history(false);
				state.prepare(&entries[..5], &[], &batches, |_| None);
				state.prepare(&entries, &groups, &Default::default(), |_| Some(88.));
				let motion = state.get(&groups[0]);
				if expanded {
					let motion = motion.expect("expanded details close with the process");
					assert!(motion.outgoing.keys().copied().eq([1]));
					assert!(motion.header_present && motion.batch(1).unwrap().expanded);
				} else {
					assert!(motion.is_none(), "a compact batch keeps the same header height");
				}
			}
		});
	}

	#[gpui::test]
	fn completing_during_tool_batch_expansion_never_adds_hidden_height(
		cx: &mut gpui::TestAppContext,
	) {
		use std::time::Duration;
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1_248.), gpui::px(840.)));
		let (binding, mut page) = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.graph_visible = false;
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap();
			work.codex_thread_id = Some("thread".into());
			let binding = Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};
			let page = decodex_protocol::AgentTimelinePage {
				thread_id: binding.thread.clone(),
				entries: vec![message(0, "userMessage", None)],
				next_cursor: None,
				source_threads: Default::default(),
				weather: Default::default(),
				thread_token_total: None,
				thread_token_breakdown: None,
				thread_metrics: None,
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None,
				observed_activities: Vec::new(),
			};
			assert!(s.timeline.native.replace(binding.clone(), page.clone()));
			cx.notify();
			(binding, page)
		});
		visual.update(crate::ui_frame::draw_frame);
		for position in 1..=6 {
			let mut step = message(position, "commandExecution", None);
			if let AgentTimelineContent::Item { activity, .. } = &mut step.content {
				*activity = Some(
					serde_json::from_value(serde_json::json!({
						"turn_id": "turn", "item_id": position.to_string(), "kind": "commandExecution",
						"status": "completed", "label": "Read layout", "detail": "", "duration_ms": null
					}))
					.unwrap(),
				);
			}
			page.entries.push(step);
		}
		surface.update(visual, |s, cx| {
			assert!(s.refresh_native_history(binding.clone(), page.clone()));
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		thread::sleep(Duration::from_millis(270));
		visual.update(crate::ui_frame::draw_frame);
		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();
		assert_eq!(
			visual.debug_bounds("tool-batch-block").unwrap().size.height,
			toggle.size.height
		);
		assert!(visual.debug_bounds("archived-process-content").is_none());
		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		visual.executor().advance_clock(Duration::from_millis(50));
		visual.update(crate::ui_frame::draw_frame);
		let opened =
			visual.debug_bounds("tool-batch-block").unwrap().size.height - toggle.size.height;
		assert!(opened > gpui::px(0.));

		page.entries.push(message(7, "agentMessage", Some("final_answer")));
		page.entries.push(AgentTimelineEntry { position: 8, ..completed("completed") });
		surface.update(visual, |s, cx| {
			assert!(s.refresh_native_history(binding, page));
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		let initial = visual.debug_bounds("turn-process-finishing").unwrap().size.height;
		assert!((initial - opened).abs() <= gpui::px(1.), "{opened:?} -> {initial:?}");
		visual.update(crate::ui_frame::draw_frame);
		thread::sleep(Duration::from_millis(90));
		visual.update(crate::ui_frame::draw_frame);
		let middle = visual.debug_bounds("turn-process-finishing").unwrap().size.height;
		assert!(middle > gpui::px(0.) && middle < initial, "{initial:?} -> {middle:?}");
		thread::sleep(Duration::from_millis(280));
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("turn-process-finishing").is_none());
		assert_eq!(
			visual.debug_bounds("turn-process-toggle").unwrap().size.height,
			toggle.size.height
		);
	}

	#[gpui::test]
	fn live_messages_keep_their_style_until_the_completed_process_finishes_closing(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		let entries = vec![
			message(0, "userMessage", None),
			message(1, "agentMessage", Some("commentary")),
			message(2, "reasoning", None),
			message(3, "agentMessage", Some("final_answer")),
		];
		let (binding, mut page) = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let binding = Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};
			let page = decodex_protocol::AgentTimelinePage {
				source_threads: Default::default(),
				thread_id: "thread".into(),
				entries,
				next_cursor: None,
				weather: Default::default(),
				thread_token_total: None,
				thread_token_breakdown: None,
				thread_metrics: None,
				safety_buffering_turn_id: None,
				observed_activities: Vec::new(),
				active_realtime_session_at_page_start: None,
			};

			assert!(s.timeline.native.replace(binding.clone(), page.clone()));

			cx.notify();

			(binding, page)
		});

		visual.update(crate::ui_frame::draw_frame);
		thread::sleep(std::time::Duration::from_millis(240));
		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("turn-process-toggle").is_none());
		assert!(visual.debug_bounds("archived-process-content").is_none());
		assert!(visual.debug_bounds("native-reasoning-summary").is_some());

		page.entries.push(completed("completed"));
		surface.update(visual, |s, cx| {
			// Reading above the tail must not turn automatic folding into a
			// permanent explicit expansion when the completion arrives.
			s.timeline.follow_paused.insert(binding.work.clone());
			assert!(s.refresh_native_history(binding, page));
			assert!(s.timeline.native.expanded_turns.is_empty());

			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);

		let initial = visual.debug_bounds("turn-process-finishing").unwrap().size.height;

		assert!(initial > gpui::px(0.));
		assert!(visual.debug_bounds("archived-process-content").is_none());

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(90));

		visual.update(crate::ui_frame::draw_frame);

		let middle = visual.debug_bounds("turn-process-finishing").unwrap().size.height;

		assert!(middle > gpui::px(0.) && middle < initial, "{initial:?} -> {middle:?}");
		assert!(visual.debug_bounds("archived-process-content").is_none());

		thread::sleep(std::time::Duration::from_millis(300));

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("turn-process-finishing").is_none());
		assert!(visual.debug_bounds("native-reasoning-summary").is_none());
		assert!(visual.debug_bounds("native-promotion-content").is_some());
		let process = visual.debug_bounds("turn-process-toggle").unwrap();
		let answer = visual.debug_bounds("native-promotion-content").unwrap();
		let gap = answer.top() - process.bottom();
		assert!(
			(gap - gpui::px(crate::ui_theme::PROCESS_CONTENT_GAP)).abs() <= gpui::px(1.),
			"steps must stay attached to their answer, got {gap:?}"
		);

		visual.update(crate::ui_frame::draw_frame);

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		assert!(surface.update(visual, |s, _| s.timeline.native.expanded_turns.contains("turn")));
		assert!(visual.debug_bounds("archived-process-content").is_some());
		assert!(visual.debug_bounds("native-reasoning-summary").is_some());
	}

	#[gpui::test]
	fn completed_process_can_be_opened_without_replacing_the_final_reply(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let mut timeline = Timeline::default();

			assert!(timeline.replace(
				Binding {
					work: work.id.clone(),
					thread: "thread".into(),
					account: "account".into()
				},
				decodex_protocol::AgentTimelinePage {
					source_threads: Default::default(),
					thread_id: "thread".into(),
					entries: vec![
						message(0, "userMessage", None),
						message(1, "reasoning", None),
						steer_with_attachment(2),
						message(3, "agentMessage", Some("final_answer")),
						completed("completed")
					],
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_token_breakdown: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					observed_activities: Vec::new(),
					active_realtime_session_at_page_start: None,
				}
			));

			s.timeline.native = timeline;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("native-reasoning-summary").is_none());
		assert!(visual.debug_bounds("native-promotion-content").is_some());

		let prompt = r#"message-["agent","thread",[0,1,"0"]]-0"#;
		let steer = r#"message-["agent","thread",[2,1,"2"]]-0"#;
		assert!(visual.debug_bounds(prompt).is_some());
		assert!(visual.debug_bounds(steer).is_none());
		assert!(visual.debug_bounds("native-media-action").is_none());

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("native-reasoning-summary").is_some());
		assert!(visual.debug_bounds("native-promotion-content").is_some());

		surface.update(visual, |_, cx| cx.notify());
		visual.update(crate::ui_frame::draw_frame);

		assert!(
			visual.debug_bounds("native-reasoning-summary").is_some(),
			"refresh preserves an explicit expansion"
		);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_frame);

		let original = visual.debug_bounds(prompt).unwrap();
		let intermediate = visual
			.debug_bounds(steer)
			.expect("the exact steer uses the native user message renderer");
		assert!((original.right() - intermediate.right()).abs() < gpui::px(2.));
		assert!(visual.debug_bounds("native-media-action").is_some());
		let process = visual.debug_bounds("native-reasoning-summary").unwrap();
		let header = visual.debug_bounds("turn-process-toggle").unwrap();

		assert!(
			process.origin.y >= header.origin.y + header.size.height,
			"disclosed content must appear below its control"
		);

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		assert!(
			surface.update(visual, |s, _| s.timeline.native.expanded_turns.contains("turn")),
			"reversal reopens the same native turn"
		);

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("native-reasoning-summary").is_none());
		assert!(visual.debug_bounds(steer).is_none());
		assert!(visual.debug_bounds(prompt).is_some());
		let mark = visual
			.debug_bounds(r#"history-tick-native-["thread",2,1,"2"]"#)
			.expect("steer navigation target");
		visual.simulate_click(mark.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds(steer).is_some(), "navigation reveals the original steer");
	}
}
