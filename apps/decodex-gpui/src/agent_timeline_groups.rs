//! Present completed native turns without discarding their process records.
use super::{AgentSurface, AgentTimelineEntry, Content};
use gpui::{AnyElement, Context, Role, SharedString, div, prelude::*, px, rgb, rgba};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Group {
	pub turn: String,
	pub indices: Vec<usize>,
	pub expanded: bool,
}

pub(super) fn groups(entries: &[AgentTimelineEntry], expanded: &BTreeSet<String>) -> Vec<Group> {
	let finished: BTreeSet<_> = entries
		.iter()
		.filter_map(|e| match &e.content {
			Content::TurnBoundary { turn_id, completed: true, status, error: None, .. }
				if status.as_deref() == Some("completed") =>
				Some(turn_id.as_str()),
			_ => None,
		})
		.collect();
	let finals: BTreeSet<_> = entries
		.iter()
		.filter_map(|e| match &e.content {
			Content::Item { turn_id, kind, phase, text, .. }
				if kind == "agentMessage"
					&& phase.as_deref() == Some("final_answer")
					&& !text.trim().is_empty() =>
				Some(turn_id.as_str()),
			_ => None,
		})
		.collect();
	let mut steps: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
	for (index, entry) in entries.iter().enumerate() {
		if let Content::Item { turn_id, kind, phase, app_ui: false, attachments, activity, .. } =
			&entry.content
			&& finished.contains(turn_id.as_str())
			&& finals.contains(turn_id.as_str())
			&& attachments.is_empty()
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
			let mut segments: Vec<Group> = Vec::new();
			for index in indices {
				if let Some(last) = segments.last_mut()
					&& last.indices.last() == index.checked_sub(1).as_ref()
				{
					last.indices.push(index);
				} else {
					segments.push(Group {
						turn: turn.into(),
						indices: vec![index],
						expanded: expanded.contains(turn),
					});
				}
			}
			segments
		})
		.collect()
}

impl AgentSurface {
	pub(super) fn turn_process_header(
		&self,
		work: &super::AgentWorkItemDto,
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
		let control = div()
			.id(SharedString::from(format!("turn-process-{identity}")))
			.debug_selector(|| "turn-process-toggle".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_expanded(expanded)
			.aria_label(format!("{} process steps", group.indices.len()))
			.cursor_pointer()
			.flex()
			.items_center()
			.gap(px(6.))
			.rounded(px(6.))
			.px(px(6.))
			.py(px(5.))
			.text_size(px(12.))
			.line_height(px(18.))
			.text_color(rgb(crate::ui_theme::TEXT_MUTED))
			.hover(|s| s.bg(rgba(crate::ui_theme::HOVER_FILL)))
			.child(crate::shell::workspace_symbols::process_chevron(
				SharedString::from(format!("turn-chevron-{identity}")),
				expanded,
			))
			.child(format!(
				"{} {}",
				group.indices.len(),
				if group.indices.len() == 1 { "step" } else { "steps" }
			))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.anchor_process_toggle(&work_id, &entry);
				if !s.native_history.expanded_turns.remove(&turn) {
					s.native_history.expanded_turns.insert(turn.clone());
				}
				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) && !event.is_held {
					s.anchor_process_toggle(&keyboard.0, &keyboard.1);
					if !s.native_history.expanded_turns.remove(&keyboard.2) {
						s.native_history.expanded_turns.insert(keyboard.2.clone());
					}
					cx.notify();
					cx.stop_propagation();
				}
			}))
			.into_any_element();
		div().flex().child(control).into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn message(index: u64, kind: &str, phase: Option<&str>) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: index,
			content: Content::Item {
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
			content: Content::TurnBoundary {
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
	fn only_finished_turns_with_explicit_final_answers_fold() {
		let mut entries = vec![
			message(0, "userMessage", None),
			message(1, "agentMessage", Some("commentary")),
			message(2, "reasoning", None),
			message(3, "agentMessage", Some("final_answer")),
		];
		assert!(
			groups(&entries, &BTreeSet::new()).is_empty(),
			"final output can arrive before work finishes"
		);
		entries.push(completed("completed"));
		let result = groups(&entries, &BTreeSet::new());
		assert_eq!(result[0].indices, vec![1, 2]);
		assert!(!result[0].expanded);
		assert!(groups(&entries, &BTreeSet::from(["turn".into()]))[0].expanded);
		entries[4] = completed("failed");
		assert!(groups(&entries, &BTreeSet::new()).is_empty());
		entries[4] = completed("interrupted");
		assert!(groups(&entries, &BTreeSet::new()).is_empty());
	}
	#[test]
	fn unknown_phases_and_interactive_content_remain_visible() {
		let mut entries = vec![
			message(0, "agentMessage", None),
			message(1, "reasoning", None),
			message(2, "agentMessage", None),
			completed("completed"),
		];
		assert!(groups(&entries, &BTreeSet::new()).is_empty());
		entries[2] = message(2, "agentMessage", Some("final_answer"));
		assert_eq!(groups(&entries, &BTreeSet::new())[0].indices, vec![1]);
		if let Content::Item { app_ui, .. } = &mut entries[1].content {
			*app_ui = true;
		}
		assert!(groups(&entries, &BTreeSet::new()).is_empty());
	}
	#[test]
	fn interleaved_user_input_splits_process_segments_without_reordering() {
		let entries = vec![
			message(0, "userMessage", None),
			message(1, "reasoning", None),
			message(2, "userMessage", None),
			message(3, "reasoning", None),
			message(4, "agentMessage", Some("final_answer")),
			completed("completed"),
		];
		let result = groups(&entries, &BTreeSet::new());
		assert_eq!(
			result.iter().map(|g| g.indices.clone()).collect::<Vec<_>>(),
			vec![vec![1], vec![3]]
		);
	}

	#[gpui::test]
	fn completed_process_can_be_opened_without_replacing_the_final_reply(
		cx: &mut gpui::TestAppContext,
	) {
		use super::super::{Binding, Timeline};
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(px(1400.), px(1400.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.graph_visible = false;
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
					active_realtime_session_at_page_start: None,
				}
			));
			s.native_history = timeline;
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear());
		std::thread::sleep(std::time::Duration::from_millis(240));
		visual.update(|w, cx| w.draw(cx).clear());
		assert!(visual.debug_bounds("native-reasoning-summary").is_none());
		assert!(visual.debug_bounds("native-promotion-content").is_some());
		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();
		assert!(toggle.size.width < px(180.), "hover stays local to the disclosure control");
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
		let initial = visual.debug_bounds("turn-process-block").unwrap().size.height;
		std::thread::sleep(std::time::Duration::from_millis(60));
		visual.update(|w, cx| w.draw(cx).clear());
		let middle = visual.debug_bounds("turn-process-block").unwrap().size.height;
		std::thread::sleep(std::time::Duration::from_millis(180));
		visual.update(|w, cx| w.draw(cx).clear());
		let full = visual.debug_bounds("turn-process-block").unwrap().size.height;
		assert!(
			middle >= initial && full >= middle && full > toggle.size.height,
			"height remains monotonic and reaches the expanded content: {initial:?} -> {middle:?} -> {full:?}"
		);
		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();
		assert!(toggle.size.width < px(180.), "hover stays local to the disclosure control");
		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|w, cx| w.draw(cx).clear());
		assert!(
			visual.debug_bounds("native-reasoning-summary").is_some(),
			"content remains mounted during exit"
		);
		std::thread::sleep(std::time::Duration::from_millis(50));
		visual.update(|w, cx| w.draw(cx).clear());
		let closing = visual.debug_bounds("turn-process-block").unwrap().size.height;
		assert!(closing <= full && closing >= toggle.size.height);
		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|w, cx| w.draw(cx).clear());
		assert!(
			visual.debug_bounds("native-reasoning-summary").is_some(),
			"reversal retains the same content"
		);
		std::thread::sleep(std::time::Duration::from_millis(240));
		visual.update(|w, cx| w.draw(cx).clear());
		assert_eq!(visual.debug_bounds("turn-process-block").unwrap().size.height, full);
		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();
		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|w, cx| w.draw(cx).clear());
		std::thread::sleep(std::time::Duration::from_millis(240));
		visual.update(|w, cx| w.draw(cx).clear());
		assert!(visual.debug_bounds("native-reasoning-summary").is_none());
	}
}
