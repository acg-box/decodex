//! Present completed native turns without discarding their process records.
use std::collections::{BTreeMap, BTreeSet};

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
	pub indices: Vec<usize>,
	pub expanded: bool,
	pub first_index: usize,
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
		let work_id = work.id.clone();
		let entry = entry.clone();
		let expanded = group.expanded;
		let keyboard = (work_id.clone(), entry.clone(), turn.clone());
		let control = gpui::div()
			.id(SharedString::from(format!("turn-process-{identity}")))
			.debug_selector(|| "turn-process-toggle".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_expanded(expanded)
			.aria_label(earlier_messages_label(group.count))
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
			.child(earlier_messages_label(group.count))
			.child(workspace_symbols::process_chevron(
				SharedString::from(format!("turn-chevron-{identity}")),
				expanded,
			))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.anchor_process_toggle(&work_id, &entry);

				if !s.timeline.native.expanded_turns.remove(&turn) {
					s.timeline.native.expanded_turns.insert(turn.clone());
				}

				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) && !event.is_held {
					s.anchor_process_toggle(&keyboard.0, &keyboard.1);

					if !s.timeline.native.expanded_turns.remove(&keyboard.2) {
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

/// Hide only empty terminal reasoning. Keep live, truncated and attachment-bearing rows.
pub(super) fn empty_completed_reasoning(entries: &[AgentTimelineEntry]) -> BTreeSet<usize> {
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
			} if kind == "reasoning"
				&& text.trim().is_empty()
				&& attachments.is_empty()
				&& completed.contains(turn_id.as_str()) =>
				Some(index),
			_ => None,
		})
		.collect()
}

pub(super) fn groups(entries: &[AgentTimelineEntry], expanded: &BTreeSet<String>) -> Vec<Group> {
	let empty = empty_completed_reasoning(entries);
	let finished: BTreeSet<_> = entries
		.iter()
		.filter_map(|e| match &e.content {
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
	let finals: BTreeSet<_> = entries
		.iter()
		.filter_map(|e| match &e.content {
			AgentTimelineContent::Item { turn_id, kind, phase, text, .. }
				if kind == "agentMessage"
					&& phase.as_deref() == Some("final_answer")
					&& !text.trim().is_empty() =>
				Some(turn_id.as_str()),
			_ => None,
		})
		.collect();
	let mut steps: BTreeMap<&str, Vec<usize>> = BTreeMap::new();

	for (index, entry) in entries.iter().enumerate() {
		if empty.contains(&index) {
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
		} = &entry.content
			&& finished.contains(turn_id.as_str())
			&& finals.contains(turn_id.as_str())
			&& attachments.is_empty()
			&& kind != "agentInput"
			&& (matches!(kind.as_str(), "reasoning" | "plan")
				|| (kind == "agentMessage" && phase.as_deref() == Some("commentary"))
				|| activity.as_ref().is_some_and(|a| {
					matches!(a.status.as_str(), "completed" | "failed" | "exited")
				})) {
			steps.entry(turn_id).or_default().push(index);
		}
	}

	// Keep interleaved user messages and interactive items in source order.
	steps
		.into_iter()
		.flat_map(|(turn, indices)| {
			let first_index = indices[0];
			let count = indices.len();
			let mut segments: Vec<Group> = Vec::new();

			for index in indices {
				if let Some(last) = segments.last_mut()
					&& last.indices.last() == index.checked_sub(1).as_ref()
				{
					last.indices.push(index);
				} else {
					segments.push(Group {
						turn: turn.into(),
						first_index,
						count,
						indices: vec![index],
						expanded: expanded.contains(turn),
					});
				}
			}

			segments
		})
		.collect()
}

fn earlier_messages_label(count: usize) -> String {
	format!("{count} earlier {}", if count == 1 { "message" } else { "messages" })
}

#[cfg(test)]
mod tests {
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

		assert!(groups::empty_completed_reasoning(&entries).is_empty());

		entries.push(completed("completed"));

		assert_eq!(groups::empty_completed_reasoning(&entries), BTreeSet::from([0]));
		assert!(groups::groups(&entries, &BTreeSet::new()).is_empty());

		if let AgentTimelineContent::Item { truncated, .. } = &mut entries[0].content {
			*truncated = true;
		}

		assert!(groups::empty_completed_reasoning(&entries).is_empty());

		if let AgentTimelineContent::Item { text, truncated, .. } = &mut entries[0].content {
			*text = "Retained summary".into();
			*truncated = false;
		}

		assert!(groups::empty_completed_reasoning(&entries).is_empty());
		assert_eq!(groups::groups(&entries, &BTreeSet::new())[0].count, 1);
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
	fn interleaved_user_input_splits_process_segments_without_reordering() {
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
				BTreeSet::from([1])
			);
			assert!(result.iter().all(|g| g.count == 2 && g.turn == "turn"));
			assert_eq!(groups::earlier_messages_label(result[0].count), "2 earlier messages");
			assert_eq!(groups::earlier_messages_label(1), "1 earlier message");
			assert_eq!(
				result.iter().map(|g| g.indices.clone()).collect::<Vec<_>>(),
				vec![vec![1], vec![3]]
			);
		}
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
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));

			s.timeline.native = timeline;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear());

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear());

		assert!(visual.debug_bounds("native-reasoning-summary").is_none());
		assert!(visual.debug_bounds("native-promotion-content").is_some());

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|w, cx| w.draw(cx).clear());

		assert!(visual.debug_bounds("native-reasoning-summary").is_some());
		assert!(visual.debug_bounds("native-promotion-content").is_some());

		surface.update(visual, |_, cx| cx.notify());
		visual.update(|w, cx| w.draw(cx).clear());

		assert!(
			visual.debug_bounds("native-reasoning-summary").is_some(),
			"refresh preserves an explicit expansion"
		);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear());

		let process = visual.debug_bounds("native-reasoning-summary").unwrap();
		let header = visual.debug_bounds("turn-process-toggle").unwrap();

		assert!(
			process.origin.y >= header.origin.y + header.size.height,
			"disclosed content must appear below its control"
		);

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|w, cx| w.draw(cx).clear());

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|w, cx| w.draw(cx).clear());

		assert!(
			surface.update(visual, |s, _| s.timeline.native.expanded_turns.contains("turn")),
			"reversal reopens the same native turn"
		);

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|w, cx| w.draw(cx).clear());

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear());

		assert!(visual.debug_bounds("native-reasoning-summary").is_none());
	}
}
