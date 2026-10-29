//! Coalesce adjacent native waits for display. Keep every source item and detail read intact.
use std::{collections::BTreeMap, mem};

use gpui::{
	AnyElement, KeyDownEvent, Role,
	prelude::{
		InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	shell::{
		agent_surface::native_timeline::{
			AgentSurface, AgentTimelineContent, AgentTimelineEntry, AgentWorkItemDto, Context,
			SharedString,
		},
		workspace_symbols,
	},
	ui_feedback, ui_motion,
	ui_theme::{self, HOVER_FILL, TEXT_MUTED},
};
use decodex_protocol::AgentActivityDto;

pub(super) struct Group {
	pub indices: Vec<usize>,
}
impl Group {
	fn summary(&self, entries: &[AgentTimelineEntry]) -> (&'static str, Option<String>) {
		let mut total = Some(0_u64);
		let mut waiting = false;

		for &index in &self.indices {
			if let AgentTimelineContent::Item { activity, .. } = &entries[index].content {
				waiting |= activity.as_ref().is_some_and(|a| a.status == "waiting");
				total = total.and_then(|total| {
					activity
						.as_ref()
						.filter(|a| a.status == "recorded")?
						.duration_ms?
						.checked_add(total)
				});
			}
		}

		(if waiting { "waiting" } else { "recorded" }, total.map(elapsed))
	}
}

impl AgentSurface {
	pub(super) fn native_wait_group(
		&self,
		work: &AgentWorkItemDto,
		group: &Group,
		anchor: &AgentTimelineEntry,
		archived: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let entries = &self.timeline.native.entries;
		let first = &entries[group.indices[0]];
		let identity =
			serde_json::json!([work.id, work.codex_thread_id, super::key(first)]).to_string();
		let key = format!("wait-group-{identity}");
		let expanded = self.timeline.expanded_records.contains(&key);
		let (status, duration) = group.summary(entries);
		let label = format!("Wait ×{}", group.indices.len());
		let action = (work.id.clone(), anchor.clone(), key.clone());
		let keyboard = action.clone();
		let header = gpui::div()
			.id(SharedString::from(key.clone()))
			.debug_selector(|| "native-wait-group-toggle".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("Inspect {} consecutive waits", group.indices.len()))
			.aria_expanded(expanded)
			.cursor_pointer()
			.w_full()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(8.))
			.py(gpui::px(5.))
			.rounded(gpui::px(6.))
			.text_size(gpui::px(12.))
			.line_height(gpui::px(18.))
			.text_color(ui_theme::rgb(TEXT_MUTED))
			.hover(|s| s.bg(ui_theme::rgba(HOVER_FILL)))
			.child(ui_feedback::execution_icon(format!("wait-status-{identity}"), status))
			.child(gpui::div().flex_1().min_w_0().child(label))
			.children(duration)
			.child(workspace_symbols::process_chevron(
				SharedString::from(format!("wait-chevron-{identity}")),
				expanded,
			))
			.on_click(cx.listener(move |s, _, _, cx| s.toggle_wait_group(&action, cx)))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) && !event.is_held {
					s.toggle_wait_group(&keyboard, cx);
					cx.stop_propagation();
				}
			}));
		let owner = cx.entity();
		let work = work.clone();
		let indices = group.indices.clone();

		gpui::div()
			.w_full()
			.min_w_0()
			.child(header)
			.child(ui_motion::disclosure_lazy(
				SharedString::from(format!("wait-body-{key}")),
				expanded,
				move |cx| {
					owner.update(cx, |s, cx| {
						gpui::div()
							.debug_selector(|| "native-wait-group-items".into())
							.w_full()
							.min_w_0()
							.pl(gpui::px(20.))
							.flex()
							.flex_col()
							.gap_2()
							.children(indices.iter().map(|&index| {
								let entry = &s.timeline.native.entries[index];
								let identity = serde_json::json!([
									work.id,
									work.codex_thread_id,
									super::key(entry)
								])
								.to_string();
								let body = s
									.native_timeline_content(&work, entry, &identity, archived, cx);

								if archived {
									body
								} else {
									s.timeline.native.arrivals.wrap(entry, body)
								}
							}))
							.into_any_element()
					})
				},
			))
			.into_any_element()
	}

	fn toggle_wait_group(
		&mut self,
		action: &(String, AgentTimelineEntry, String),
		cx: &mut Context<Self>,
	) {
		self.anchor_process_toggle(&action.0, &action.1);

		if !self.timeline.expanded_records.remove(&action.2) {
			self.timeline.expanded_records.insert(action.2.clone());
		}

		cx.notify();
	}
}

/// Never merge across input, other activity, or a native turn boundary.
pub(super) fn groups(entries: &[AgentTimelineEntry], indices: &[usize]) -> BTreeMap<usize, Group> {
	let mut result = BTreeMap::new();
	let mut run = Vec::new();

	for &index in indices {
		let turn = wait(&entries[index]);
		let continues = turn.is_some()
			&& run
				.last()
				.is_none_or(|&previous| previous + 1 == index && wait(&entries[previous]) == turn);

		if !continues && !run.is_empty() {
			if run.len() > 1 {
				result.insert(run[0], Group { indices: mem::take(&mut run) });
			}

			run.clear();
		}
		if turn.is_some() {
			run.push(index);
		}
	}

	if run.len() > 1 {
		result.insert(run[0], Group { indices: run });
	}

	result
}

/// Requested maximum and observed elapsed time are different facts, not two durations to display.
pub(super) fn timing(activity: &AgentActivityDto) -> Option<String> {
	if activity.status == "recorded"
		&& let Some(ms) = activity.duration_ms
	{
		return Some(elapsed(ms));
	}

	(!activity.detail.is_empty()).then(|| activity.detail.clone())
}

fn wait(entry: &AgentTimelineEntry) -> Option<&str> {
	match &entry.content {
		AgentTimelineContent::Item {
			turn_id,
			kind,
			text,
			truncated: false,
			app_ui: false,
			attachments,
			collaboration: None,
			activity,
			..
		} if kind == "sleep"
			&& text.trim().is_empty()
			&& attachments.is_empty()
			&& activity.as_ref().is_none_or(|a| {
				a.kind == "sleep" && matches!(a.status.as_str(), "waiting" | "recorded")
			}) =>
			Some(turn_id),
		_ => None,
	}
}

fn elapsed(ms: u64) -> String {
	if ms < 1_000 {
		format!("{ms}ms")
	} else {
		let seconds = format!("{:.1}", ms as f64 / 1_000.);

		format!("{}s", seconds.strip_suffix(".0").unwrap_or(&seconds))
	}
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::native_timeline::waits::{
		self, AgentActivityDto, AgentSurface, AgentTimelineContent, AgentTimelineEntry,
	};

	fn item(index: u64, kind: &str) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: index,
			content: AgentTimelineContent::Item {
				turn_id: "turn".into(),
				item_id: index.to_string(),
				kind: kind.into(),
				phase: None,
				text: String::new(),
				truncated: false,
				app_ui: false,
				attachments: vec![],
				collaboration: None,
				activity: Some(AgentActivityDto {
					retained_output_lines: None,
					review: None,
					turn_id: "turn".into(),
					item_id: index.to_string(),
					kind: kind.into(),
					status: "recorded".into(),
					label: "Wait".into(),
					detail: "Up to 60s".into(),
					duration_ms: Some(60_002),
					plugin_id: None,
					read_only_hint: None,
					native_timestamp_ms: None,
				}),
			},
		}
	}

	#[test]
	fn wait_summary_uses_elapsed_once_and_preserves_a_requested_limit_when_unknown() {
		let AgentTimelineContent::Item { mut activity, .. } = item(0, "sleep").content else {
			unreachable!()
		};
		let a = activity.as_mut().unwrap();

		assert_eq!(waits::timing(a).as_deref(), Some("60s"));

		a.duration_ms = Some(7_342);

		assert_eq!(waits::timing(a).as_deref(), Some("7.3s"));

		a.duration_ms = None;

		assert_eq!(waits::timing(a).as_deref(), Some("Up to 60s"));

		a.status = "waiting".into();
		a.duration_ms = Some(60_000);

		assert_eq!(waits::timing(a).as_deref(), Some("Up to 60s"));
	}

	#[test]
	fn consecutive_waits_merge_without_absorbing_messages_other_tools_or_other_turns() {
		let mut entries: Vec<_> = (0..9).map(|i| item(i, "sleep")).collect();

		entries[2] = item(2, "reasoning");
		entries[5] = item(5, "commandExecution");

		if let AgentTimelineContent::Item { turn_id, .. } = &mut entries[8].content {
			*turn_id = "next".into();
		}

		let runs = waits::groups(&entries, &(0..entries.len()).collect::<Vec<_>>());

		assert_eq!(
			runs.values().map(|g| g.indices.clone()).collect::<Vec<_>>(),
			[vec![0, 1], vec![3, 4], vec![6, 7]]
		);
		assert!(
			waits::groups(&entries, &[0, 3]).is_empty(),
			"a process subset cannot hide intervening records"
		);
		assert_eq!(runs[&0].summary(&entries), ("recorded", Some("120s".into())));
	}

	#[test]
	fn unknown_or_pending_waits_do_not_invent_an_elapsed_total_or_a_success() {
		let mut entries = vec![item(0, "sleep"), item(1, "sleep")];

		if let AgentTimelineContent::Item { activity, .. } = &mut entries[1].content {
			activity.as_mut().unwrap().status = "waiting".into();
			activity.as_mut().unwrap().duration_ms = None;
		}

		let runs = waits::groups(&entries, &[0, 1]);

		assert_eq!(runs[&0].summary(&entries), ("waiting", None));

		if let AgentTimelineContent::Item { activity, .. } = &mut entries[1].content {
			*activity = None;
		}

		assert_eq!(runs[&0].summary(&entries), ("recorded", None));

		if let AgentTimelineContent::Item { text, .. } = &mut entries[1].content {
			*text = "Could not wait".into();
		}

		assert!(
			waits::groups(&entries, &[0, 1]).is_empty(),
			"retain an unexpected outcome visibly"
		);
	}

	#[gpui::test]
	fn wait_disclosure_survives_appends_and_the_completed_process_keeps_exact_items(
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
			s.timeline.native.binding = Some(super::super::Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			});
			s.timeline.native.entries = vec![item(0, "sleep"), item(1, "sleep")];

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		let toggle = visual.debug_bounds("native-wait-group-toggle").unwrap();

		assert!(visual.debug_bounds("native-wait-group-items").is_none());

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("native-wait-group-items").is_some());

		surface.update(visual, |s, cx| {
			s.timeline.native.entries.push(item(2, "sleep"));
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("native-wait-group-items").is_some());

		surface.update(visual, |s, cx| {
			let mut final_answer = item(3, "agentMessage");

			if let AgentTimelineContent::Item { phase, text, activity, .. } =
				&mut final_answer.content
			{
				*phase = Some("final_answer".into());
				*text = "Ready.".into();
				*activity = None;
			}

			s.timeline.native.entries.push(final_answer);
			s.timeline.native.entries.push(AgentTimelineEntry {
				position: 4,
				content: AgentTimelineContent::TurnBoundary {
					turn_id: "turn".into(),
					completed: true,
					status: Some("completed".into()),
					duration_ms: Some(180_006),
					timing: None,
					usage_summary: None,
					usage: None,
					error: None,
				},
			});
			s.timeline.native.expanded_turns.insert("turn".into());
			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("archived-process-content").is_some());
		assert!(visual.debug_bounds("native-wait-group-items").is_some());

		surface.update(visual, |s, _| {
			assert_eq!(s.timeline.native.entries.len(), 5);

			for (index, entry) in s.timeline.native.entries[..3].iter().enumerate() {
				assert_eq!(*entry, item(index as u64, "sleep"));
			}
		});
	}
}
