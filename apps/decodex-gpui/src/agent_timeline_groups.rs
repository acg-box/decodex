//! Present completed native turns without discarding their process records.
use std::collections::BTreeSet;

use gpui::{
	AnyElement, Context, FontWeight, KeyDownEvent, Role, SharedString,
	prelude::{
		InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	shell::{
		agent_surface::native_timeline::{
			AgentSurface, AgentTimelineContent, AgentTimelineEntry, AgentWorkItemDto,
		},
		workspace_symbols,
	},
	ui_theme::{TEXT, TEXT_MUTED},
};

pub(super) struct Group {
	pub turn: String,
	pub turns: BTreeSet<String>,
	pub indices: Vec<usize>,
	pub expanded: bool,
	pub first_index: usize,
	pub final_index: usize,
	pub count: usize,
}

impl AgentSurface {
	pub(super) fn turn_process_header(
		&self,
		work: &AgentWorkItemDto,
		group: &Group,
		entry: &AgentTimelineEntry,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let identity =
			serde_json::json!([work.id, work.codex_thread_id, super::key(entry)]).to_string();
		let turn = group.turn.clone();
		let turns = group.turns.clone();
		let work_id = work.id.clone();
		let entry = entry.clone();
		let expanded = group.expanded;
		let keyboard = (work_id.clone(), entry.clone(), turn.clone(), turns.clone());
		let control = gpui::div()
			.id(SharedString::from(format!("turn-process-{identity}")))
			.debug_selector(|| "turn-process-toggle".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_expanded(expanded)
			.aria_label(process_label(group.count))
			.cursor_pointer()
			.flex()
			.items_center()
			.gap(gpui::px(6.))
			.py(gpui::px(5.))
			.text_size(gpui::px(13.))
			.font_weight(FontWeight::NORMAL)
			.line_height(gpui::px(18.))
			.text_color(gpui::rgb(TEXT_MUTED))
			.hover(|s| s.text_color(gpui::rgb(TEXT)))
			.child(process_label(group.count))
			.child(workspace_symbols::process_chevron(
				SharedString::from(format!("turn-chevron-{identity}")),
				expanded,
			))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.anchor_process_toggle(&work_id, &entry);

				s.timeline.native.expanded_turns.retain(|id| !turns.contains(id));

				if !expanded {
					s.timeline.native.expanded_turns.insert(turn.clone());
				}

				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) && !event.is_held {
					s.anchor_process_toggle(&keyboard.0, &keyboard.1);

					s.timeline.native.expanded_turns.retain(|id| !keyboard.3.contains(id));

					if !expanded {
						s.timeline.native.expanded_turns.insert(keyboard.2.clone());
					}

					cx.notify();
					cx.stop_propagation();
				}
			}))
			.into_any_element();

		gpui::div().flex().child(control).into_any_element()
	}
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
				app_ui: false,
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
pub(super) fn groups(entries: &[AgentTimelineEntry], expanded: &BTreeSet<String>) -> Vec<Group> {
	let empty = hidden_items(entries);
	let finished: BTreeSet<_> = entries
		.iter()
		.filter_map(|entry| match &entry.content {
			AgentTimelineContent::TurnBoundary {
				turn_id,
				completed: true,
				status,
				error: None,
				..
			} if status.as_deref() == Some("completed") => Some(turn_id.as_str()),
			_ => None,
		})
		.collect();
	let mut starts = vec![0];
	for (index, entry) in entries.iter().enumerate().skip(1) {
		if matches!(&entry.content, AgentTimelineContent::Item { kind, .. }
			if matches!(kind.as_str(), "userMessage" | "agentInput"))
			|| matches!(
				&entry.content,
				AgentTimelineContent::Speech { .. }
					| AgentTimelineContent::VoiceBoundary { .. }
					| AgentTimelineContent::Promotion { .. }
			) {
			starts.push(index);
		}
	}
	starts.push(entries.len());
	let mut result = Vec::new();
	for range in starts.windows(2) {
		let range = range[0]..range[1];
		let final_index = range.clone().rev().find(|index| {
			matches!(&entries[*index].content,
				AgentTimelineContent::Item { turn_id, kind, phase, text, .. }
				if finished.contains(turn_id.as_str()) && kind == "agentMessage"
					&& phase.as_deref() == Some("final_answer") && !text.trim().is_empty())
		});
		let Some(final_index) = final_index else { continue };
		let mut indices = Vec::new();
		let mut turns = BTreeSet::new();
		for index in range {
			if empty.contains(&index) || index == final_index {
				continue;
			}
			if let AgentTimelineContent::Item {
				turn_id,
				kind,
				phase,
				app_ui: false,
				attachments,
				activity,
				..
			} = &entries[index].content
				&& finished.contains(turn_id.as_str())
				&& attachments.is_empty()
				&& (matches!(kind.as_str(), "reasoning" | "plan" | "functionCallOutput" | "sleep")
					|| (kind == "agentMessage"
						&& matches!(phase.as_deref(), Some("commentary" | "final_answer")))
					|| (kind != "agentInput" && activity.is_some()))
			{
				indices.push(index);
				turns.insert(turn_id.clone());
			}
		}
		if let Some(&first_index) = indices.first() {
			let AgentTimelineContent::Item { turn_id, .. } = &entries[first_index].content else {
				unreachable!("process contains items");
			};
			result.push(Group {
				turn: turn_id.clone(),
				expanded: !expanded.is_disjoint(&turns),
				turns,
				first_index: first_index.min(final_index),
				final_index,
				count: indices.len(),
				indices,
			});
		}
	}
	result
}

fn process_label(count: usize) -> String {
	format!("{count} {}", if count == 1 { "step" } else { "steps" })
}

#[cfg(test)]
mod tests {
	use gpui::AppContext as _;
	use std::thread;

	use crate::shell::agent_surface::native_timeline::{
		Binding, Timeline,
		groups::{self, AgentSurface, AgentTimelineContent, AgentTimelineEntry, BTreeSet},
	};

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
				app_ui: false,
				attachments: vec![],
			},
		}
	}
	fn completed(status: &str) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: 4,
			content: AgentTimelineContent::TurnBoundary {
				turn_id: "turn".into(),
				completed: true,
				status: Some(status.into()),
				duration_ms: Some(500),
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
		assert_eq!(groups[0].indices, [2, 3, 4]);
		assert_eq!(groups[0].final_index, 1);
		if let AgentTimelineContent::Item { text, .. } = &mut entries[2].content {
			*text = "Could not wait for input".into();
		}
		assert!(groups::hidden_items(&entries).is_empty());
	}

	#[test]
	fn only_finished_turns_with_explicit_final_answers_fold() {
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

		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());

		entries[4] = completed("interrupted");

		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());
	}
	#[test]
	fn unknown_phases_and_interactive_content_remain_visible() {
		let mut entries = vec![
			message(0, "agentMessage", None),
			message(1, "reasoning", None),
			message(2, "agentMessage", None),
			completed("completed"),
		];

		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());

		entries[2] = message(2, "agentMessage", Some("final_answer"));

		assert_eq!(groups::groups(&entries, &BTreeSet::new())[0].indices, vec![1]);

		if let AgentTimelineContent::Item { app_ui, .. } = &mut entries[1].content {
			*app_ui = true;
		}

		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());
	}
	#[test]
	fn a_new_user_input_does_not_absorb_the_previous_unanswered_input() {
		for input_kind in ["userMessage", "agentInput"] {
			let entries = vec![
				message(0, "userMessage", None),
				message(1, "reasoning", None),
				message(2, input_kind, None),
				message(3, "reasoning", None),
				message(4, "agentMessage", Some("final_answer")),
				completed("completed"),
			];
			let result = groups::groups(&entries, &BTreeSet::new());

			assert_eq!(
				result.iter().map(|g| g.first_index).collect::<BTreeSet<_>>(),
				BTreeSet::from([3])
			);
			assert!(result.iter().all(|g| g.count == 1 && g.turn == "turn"));
			assert_eq!(groups::process_label(result[0].count), "1 step");
			assert_eq!(groups::process_label(1), "1 step");
			assert_eq!(result.iter().map(|g| g.indices.clone()).collect::<Vec<_>>(), vec![vec![3]]);
		}
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
		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());
		assert!(groups::groups(&entries[..5], &BTreeSet::new()).is_empty());
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
			state.present_live("1");
			state.replace_history(true);
			state.prepare(&entries, &groups);
			let motion = state.get(&groups[0]).unwrap();
			assert_eq!(motion.outgoing, BTreeSet::from([1]));
			state.replace_history(true);
			state.prepare(&entries, &groups);
			assert!(std::rc::Rc::ptr_eq(&motion, &state.get(&groups[0]).unwrap()));
			state.present_live("1");
			state.replace_history(false);
			state.prepare(&entries, &groups);
			assert!(state.get(&groups[0]).is_none(), "a different source is static history");
			state.prepare(&entries[..2], &[]);
			state.replace_history(true);
			state.prepare(&entries, &groups);
			assert!(
				state.get(&groups[0]).is_some(),
				"a native page replacement must retain displayed rows too"
			);
		});
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
				thread_id: "thread".into(),
				entries,
				next_cursor: None,
				weather: Default::default(),
				thread_token_total: None,
				thread_metrics: None,
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None,
			};
			assert!(s.timeline.native.replace(binding.clone(), page.clone()));
			cx.notify();
			(binding, page)
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("turn-process-toggle").is_none());
		assert!(visual.debug_bounds("archived-process-content").is_none());
		assert!(visual.debug_bounds("native-reasoning-summary").is_some());
		page.entries.push(completed("completed"));
		surface.update(visual, |s, cx| {
			assert!(s.timeline.native.refresh(binding, page));
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
					thread_id: "thread".into(),
					entries: vec![
						message(0, "userMessage", None),
						message(1, "reasoning", None),
						message(2, "agentMessage", Some("final_answer")),
						completed("completed")
					],
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
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
	}
}
