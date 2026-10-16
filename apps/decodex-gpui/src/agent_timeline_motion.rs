//! Animate appended rows once. Paging and viewport re-entry keep their exact geometry.
use std::{
	cell::RefCell,
	collections::{BTreeMap, BTreeSet},
	time::{Duration, Instant},
};

use gpui::{AnyElement, App, IntoElement, RenderOnce, SharedString, Window, prelude::*};

use super::{AgentTimelineContent, AgentTimelineEntry};
use crate::ui_motion;

const DURATION: Duration = Duration::from_millis(200);

#[derive(Default)]
pub(super) struct Arrivals {
	latest: Option<u64>,
	rows: BTreeMap<String, Instant>,
	live: RefCell<BTreeSet<String>>,
}
impl Arrivals {
	pub(super) fn observe(&mut self, entries: &[AgentTimelineEntry], animate: bool) {
		let now = Instant::now();
		self.rows.retain(|_, born| now.duration_since(*born) < DURATION);
		for entry in entries {
			if let AgentTimelineContent::Item { kind, item_id, .. } = &entry.content
				&& !self.live.get_mut().remove(item_id)
				&& animate
				&& self.latest.is_some_and(|last| entry.position > last)
				&& !matches!(kind.as_str(), "userMessage" | "agentInput")
			{
				self.rows.entry(item_id.clone()).or_insert(now);
			}
		}
		self.latest.get_or_insert(0);
		if let Some(last) = entries.last() {
			self.latest = Some(self.latest.unwrap_or(0).max(last.position));
		}
	}

	pub(super) fn present_live(&self, item: &str) {
		self.live.borrow_mut().insert(item.into());
	}

	pub(super) fn wrap(&self, entry: &AgentTimelineEntry, child: AnyElement) -> AnyElement {
		let AgentTimelineContent::Item { item_id, .. } = &entry.content else { return child };
		let Some(started) = self.rows.get(item_id).copied() else { return child };
		Row { key: format!("row-arrival-{item_id}").into(), started, child }.into_any_element()
	}
}

#[derive(IntoElement)]
struct Row {
	key: SharedString,
	started: Instant,
	child: AnyElement,
}
impl RenderOnce for Row {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.key, cx, |_, _| (0_f32, false));
		let (height, progress) = state.update(cx, |s, _| {
			let progress = progress(self.started.elapsed(), ui_motion::reduced() || s.1);
			s.1 |= progress == 1.;
			(s.0, progress)
		});
		if progress < 1. {
			ui_motion::request_frame(window, cx);
		}
		gpui::div()
			.w_full()
			.min_w_0()
			.flex_none()
			.debug_selector(|| "transcript-row-arrival".into())
			.when(progress < 1., |row| row.h(gpui::px(height * progress)).overflow_hidden())
			.opacity(progress)
			.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.flex_none()
					.on_children_prepainted(move |bounds, window, cx| {
						if progress < 1.
							&& let Some(bounds) = bounds.first()
						{
							let measured = f32::from(bounds.size.height);
							if (state.read(cx).0 - measured).abs() > 0.1 {
								state.update(cx, |s, _| s.0 = measured);
								ui_motion::request_frame(window, cx);
							}
						}
					})
					.child(self.child),
			)
	}
}

fn progress(elapsed: Duration, reduced: bool) -> f32 {
	if reduced {
		return 1.;
	}
	let t = (elapsed.as_secs_f32() / DURATION.as_secs_f32()).min(1.);
	1. - (1. - t).powi(3)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn row(position: u64) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position,
			content: AgentTimelineContent::Item {
				turn_id: "turn".into(),
				item_id: position.to_string(),
				kind: "dynamicToolCall".into(),
				phase: None,
				text: String::new(),
				truncated: false,
				activity: None,
				collaboration: None,
				app_ui: false,
				attachments: vec![],
			},
		}
	}

	#[test]
	fn only_appends_animate_and_refreshes_do_not_restart_them() {
		let mut arrivals = Arrivals::default();
		arrivals.observe(&[row(10), row(11)], true);
		assert!(arrivals.rows.is_empty(), "initial history has its own reveal");
		arrivals.observe(&[row(1), row(10), row(11)], true);
		assert!(arrivals.rows.is_empty(), "pagination preserves the reading position");
		arrivals.observe(&[row(11), row(12)], true);
		let born = arrivals.rows["12"];
		arrivals.observe(&[row(11), row(12)], true);
		assert_eq!(arrivals.rows["12"], born);
		arrivals.observe(&[row(12), row(13)], false);
		assert!(!arrivals.rows.contains_key("13"), "background updates stay static");
		assert_eq!(progress(Duration::ZERO, false), 0.);
		assert!(progress(Duration::from_millis(100), false) < 1.);
		assert_eq!(progress(DURATION, false), 1.);
		assert_eq!(progress(Duration::ZERO, true), 1.);
	}

	struct Fixture(Instant);
	impl gpui::Render for Fixture {
		fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			gpui::div().size_full().flex().flex_col().child(Row {
				key: "tool-entry".into(),
				started: self.0,
				child: gpui::div().w_full().h(gpui::px(80.)).into_any_element(),
			})
		}
	}

	#[gpui::test]
	fn new_tool_rows_grow_in_place_and_finish_at_their_natural_height(
		cx: &mut gpui::TestAppContext,
	) {
		let (_, visual) = cx.add_window_view(|_, _| Fixture(Instant::now()));
		visual.update(|window, cx| window.draw(cx).clear(cx));
		std::thread::sleep(Duration::from_millis(60));
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let middle = visual.debug_bounds("transcript-row-arrival").unwrap().size.height;
		assert!(middle > gpui::px(0.) && middle < gpui::px(80.));
		std::thread::sleep(DURATION);
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert_eq!(
			visual.debug_bounds("transcript-row-arrival").unwrap().size.height,
			gpui::px(80.)
		);
	}
}
