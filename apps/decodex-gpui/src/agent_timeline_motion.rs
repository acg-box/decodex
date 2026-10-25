//! Animate appended rows once. Paging and viewport re-entry keep their exact geometry.
use std::{
	cell::{Cell, RefCell},
	collections::{BTreeMap, BTreeSet},
	rc::Rc,
	time::{Duration, Instant},
};

use gpui::{
	AnyElement, App, IntoElement, RenderOnce, SharedString, Window,
	prelude::{FluentBuilder as _, InteractiveElement as _, ParentElement, Styled},
};

use crate::{
	frame_trace,
	shell::agent_surface::native_timeline::{AgentTimelineContent, AgentTimelineEntry},
	ui_motion,
};

const DURATION: Duration = Duration::from_millis(260);
const STAGGER: Duration = Duration::from_millis(40);
const MAX_STAGGER: Duration = Duration::from_millis(120);

#[derive(Default)]
pub(super) struct Arrivals {
	latest: Option<u64>,
	rows: BTreeMap<String, Rc<Arrival>>,
	live: RefCell<BTreeSet<String>>,
}
impl Arrivals {
	pub(super) fn observe(&mut self, entries: &[AgentTimelineEntry], animate: bool) {
		let now = Instant::now();

		self.rows.retain(|_, arrival| {
			!arrival.finished.get() && now.saturating_duration_since(arrival.started) < DURATION
		});

		let mut delay = Duration::ZERO;

		for entry in entries {
			if let AgentTimelineContent::Item { kind, item_id, .. } = &entry.content
				&& !self.live.get_mut().remove(item_id)
				&& animate
				&& self.latest.is_some_and(|last| entry.position > last)
				&& !matches!(kind.as_str(), "userMessage" | "agentInput")
			{
				frame_trace::row_arrival(entry.position, "queued", 0., 0.);

				// A native page can contain several completed tools. Keep this burst
				// ordered without making later network updates wait behind a queue.
				self.rows
					.entry(item_id.clone())
					.or_insert_with(|| Rc::new(Arrival::new(now + delay)));

				delay = (delay + STAGGER).min(MAX_STAGGER);
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

	pub(super) fn active(&self, entry: &AgentTimelineEntry) -> bool {
		matches!(&entry.content, AgentTimelineContent::Item { item_id, .. }
			if self.rows.get(item_id).is_some_and(|arrival| !arrival.finished.get() && arrival.started.elapsed() < DURATION))
	}

	pub(super) fn wrap(&self, entry: &AgentTimelineEntry, child: AnyElement) -> AnyElement {
		let AgentTimelineContent::Item { item_id, .. } = &entry.content else { return child };
		let Some(arrival) = self.rows.get(item_id).cloned() else { return child };

		Row {
			key: format!("row-arrival-{item_id}").into(),
			position: entry.position,
			arrival,
			child,
		}
		.into_any_element()
	}
}

struct Arrival {
	started: Instant,
	finished: Cell<bool>,
}
impl Arrival {
	fn new(started: Instant) -> Self {
		Self { started, finished: Cell::new(false) }
	}

	fn sample(&self, now: Instant, reduced: bool) -> f32 {
		let value =
			progress(now.saturating_duration_since(self.started), reduced || self.finished.get());

		self.finished.set(value == 1.);

		value
	}
}

#[derive(IntoElement)]
struct Row {
	key: SharedString,
	position: u64,
	arrival: Rc<Arrival>,
	child: AnyElement,
}
impl RenderOnce for Row {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.key, cx, |_, _| 0_f32);
		let height = *state.read(cx);
		let progress = self.arrival.sample(Instant::now(), ui_motion::reduced());

		if progress < 1. {
			frame_trace::row_arrival(self.position, "render", progress, height);
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

							if (*state.read(cx) - measured).abs() > 0.1 {
								state.update(cx, |s, _| *s = measured);

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

	// Space and opacity share a zero-velocity start and finish. An ease-out
	// alone moves most of a tool burst in its first few frames.
	t * t * (3. - 2. * t)
}

#[cfg(test)]
mod tests {
	use std::thread;

	use crate::shell::agent_surface::native_timeline::{
		motion::{
			self, AgentTimelineContent, AgentTimelineEntry, Arrival, Arrivals, DURATION, Duration,
			Instant, IntoElement, MAX_STAGGER, ParentElement, Rc, Row, STAGGER, Styled, Window,
		},
		render,
	};

	struct Fixture(Instant);
	impl gpui::Render for Fixture {
		fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			gpui::div().size_full().flex().flex_col().child(Row {
				key: "tool-entry".into(),
				position: 0,
				arrival: Rc::new(Arrival::new(self.0)),
				child: gpui::div().w_full().h(gpui::px(80.)).into_any_element(),
			})
		}
	}

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
	fn background_completion_does_not_replay_after_reactivation() {
		let now = Instant::now();
		let arrival = Arrival::new(now);

		assert!(arrival.sample(now + DURATION / 4, false) < 1.);
		assert_eq!(arrival.sample(now + DURATION / 3, true), 1.);
		assert_eq!(arrival.sample(now + DURATION / 2, false), 1.);
	}

	#[test]
	fn a_tool_burst_has_a_short_bounded_stagger_and_no_initial_velocity_jump() {
		let mut arrivals = Arrivals::default();

		arrivals.observe(&[row(1)], true);
		arrivals.observe(&(1..=8).map(row).collect::<Vec<_>>(), true);

		let first = arrivals.rows["2"].started;

		assert_eq!(arrivals.rows["3"].started - first, STAGGER);
		assert_eq!(arrivals.rows["8"].started - first, MAX_STAGGER);

		arrivals.observe(&(1..=8).map(row).collect::<Vec<_>>(), true);

		assert_eq!(arrivals.rows["3"].started - first, STAGGER);
		assert!(motion::progress(DURATION / 4, false) < 0.2);
		assert_eq!(motion::progress(DURATION / 2, false), 0.5);
	}

	#[test]
	fn only_appends_animate_and_refreshes_do_not_restart_them() {
		let mut arrivals = Arrivals::default();

		arrivals.observe(&[row(10), row(11)], true);

		assert!(arrivals.rows.is_empty(), "initial history has its own reveal");

		arrivals.observe(&[row(1), row(10), row(11)], true);

		assert!(arrivals.rows.is_empty(), "pagination preserves the reading position");

		arrivals.observe(&[row(11), row(12)], true);

		let born = arrivals.rows["12"].started;

		arrivals.observe(&[row(11), row(12)], true);

		assert_eq!(arrivals.rows["12"].started, born);

		arrivals.observe(&[row(12), row(13)], false);

		assert!(!arrivals.rows.contains_key("13"), "background updates stay static");
		assert_eq!(motion::progress(Duration::ZERO, false), 0.);
		assert!(motion::progress(Duration::from_millis(100), false) < 1.);
		assert_eq!(motion::progress(DURATION, false), 1.);
		assert_eq!(motion::progress(Duration::ZERO, true), 1.);
	}

	#[gpui::test]
	fn natural_tool_content_grows_before_the_arrival_finishes(cx: &mut gpui::TestAppContext) {
		struct Tool(Instant);

		impl gpui::Render for Tool {
			fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
				gpui::div().w(gpui::px(500.)).flex().flex_col().child(Row {
					key: "natural-tool".into(),
					position: 0,
					arrival: Rc::new(Arrival::new(self.0)),
					child: render::process_indent(
						gpui::div()
							.w_full()
							.flex()
							.items_center()
							.py(gpui::px(5.))
							.text_size(gpui::px(12.))
							.line_height(gpui::px(18.))
							.child("Running command"),
					),
				})
			}
		}

		let (_, visual) = cx.add_window_view(|_, _| Tool(Instant::now()));

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(Duration::from_millis(60));

		visual.update(crate::ui_frame::draw_current_frame);

		let middle = visual.debug_bounds("transcript-row-arrival").unwrap().size.height;

		assert!(middle > gpui::px(0.) && middle < gpui::px(28.), "{middle:?}");
	}

	#[gpui::test]
	fn new_tool_rows_grow_in_place_and_finish_at_their_natural_height(
		cx: &mut gpui::TestAppContext,
	) {
		let (_, visual) = cx.add_window_view(|_, _| Fixture(Instant::now()));

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(Duration::from_millis(60));

		visual.update(crate::ui_frame::draw_current_frame);

		let middle = visual.debug_bounds("transcript-row-arrival").unwrap().size.height;

		assert!(middle > gpui::px(0.) && middle < gpui::px(80.));

		thread::sleep(DURATION);

		visual.update(crate::ui_frame::draw_current_frame);

		assert_eq!(
			visual.debug_bounds("transcript-row-arrival").unwrap().size.height,
			gpui::px(80.)
		);
	}
}
