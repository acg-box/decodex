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
	shell::agent_surface::native_timeline::{AgentTimelineContent, AgentTimelineEntry, ItemKey},
	ui_motion,
};

const DURATION: Duration = Duration::from_millis(260);
const STAGGER: Duration = Duration::from_millis(40);
const MAX_STAGGER: Duration = Duration::from_millis(120);
const MIN_TOOL_BATCH: usize = 4;

#[derive(Clone, Copy)]
pub(super) struct ToolBatch {
	pub count: usize,
	pub expanded: bool,
}

#[derive(Default)]
pub(super) struct Arrivals {
	latest: Option<u64>,
	rows: BTreeMap<ItemKey, Rc<Arrival>>,
	live: RefCell<BTreeSet<ItemKey>>,
	batches: BTreeMap<ItemKey, ToolBatch>,
}
impl Arrivals {
	pub(super) fn observe(&mut self, entries: &[AgentTimelineEntry], animate: bool) {
		let now = Instant::now();

		self.rows.retain(|_, arrival| {
			!arrival.finished.get() && now.saturating_duration_since(arrival.started) < DURATION
		});

		let start = self
			.latest
			.map_or(entries.len(), |last| entries.partition_point(|entry| entry.position <= last));
		for entry in &entries[..start] {
			if let Some(key) = ItemKey::for_entry(entry) {
				self.live.get_mut().remove(&key);
			}
		}

		let mut delay = Duration::ZERO;
		for run in entries[start..].chunk_by(|a, b| {
			quiet_tool_turn(a).is_some_and(|turn| quiet_tool_turn(b) == Some(turn))
		}) {
			let mut visible = run;
			if run.len() >= MIN_TOOL_BATCH {
				let key = ItemKey::for_entry(&run[0]).expect("tool batch begins with an item");
				self.batches.insert(key, ToolBatch { count: run.len(), expanded: false });
				visible = &run[..1];
			}
			for entry in visible {
				if let AgentTimelineContent::Item { kind, turn_id, item_id, .. } = &entry.content
					&& !self.live.get_mut().remove(&ItemKey::new(turn_id, item_id))
					&& animate
					&& !matches!(kind.as_str(), "userMessage" | "agentInput")
				{
					frame_trace::row_arrival(entry.position, "queued", 0., 0.);
					self.rows
						.entry(ItemKey::new(turn_id, item_id))
						.or_insert_with(|| Rc::new(Arrival::new(now + delay)));
					delay = (delay + STAGGER).min(MAX_STAGGER);
				}
			}
		}

		self.latest.get_or_insert(0);

		if let Some(last) = entries.last() {
			self.latest = Some(self.latest.unwrap_or(0).max(last.position));
		}
	}

	pub(super) fn tool_batch(&self, entries: &[AgentTimelineEntry]) -> Option<ToolBatch> {
		if self.batches.is_empty() {
			return None;
		}
		let first = entries.first()?;
		let turn = quiet_tool_turn(first)?;
		let batch = *self.batches.get(&ItemKey::for_entry(first)?)?;
		entries
			.get(..batch.count)?
			.iter()
			.all(|entry| quiet_tool_turn(entry) == Some(turn))
			.then_some(batch)
	}

	pub(super) fn expand_tool_batch(&mut self, key: &ItemKey, expanded: bool) {
		if let Some(batch) = self.batches.get_mut(key) {
			batch.expanded = expanded;
		}
	}

	pub(super) fn leading_gap(&self, entry: &AgentTimelineEntry, gap: f32) -> f32 {
		ItemKey::for_entry(entry)
			.and_then(|key| self.rows.get(&key))
			.map_or(gap, |arrival| gap * arrival.sample(Instant::now(), ui_motion::reduced()))
	}

	pub(super) fn present_live(&self, turn: &str, item: &str) {
		self.live.borrow_mut().insert(ItemKey::new(turn, item));
	}

	pub(super) fn active(&self, entry: &AgentTimelineEntry) -> bool {
		ItemKey::for_entry(entry).is_some_and(|key| {
			self.rows.get(&key).is_some_and(|arrival| {
				!arrival.finished.get() && arrival.started.elapsed() < DURATION
			})
		})
	}

	pub(super) fn wrap(&self, entry: &AgentTimelineEntry, child: AnyElement) -> AnyElement {
		let Some(key) = ItemKey::for_entry(entry) else { return child };
		let Some(arrival) = self.rows.get(&key).cloned() else { return child };

		Row { key: format!("row-arrival-{key:?}").into(), position: entry.position, arrival, child }
			.into_any_element()
	}
}

// Keep running work, failures, review activity, and anything with an attachment in view.
fn quiet_tool_turn(entry: &AgentTimelineEntry) -> Option<&str> {
	match &entry.content {
		AgentTimelineContent::Item {
			turn_id, kind, activity: Some(activity), attachments, ..
		} if attachments.is_empty()
			&& activity.review.is_none()
			&& matches!(activity.status.as_str(), "completed" | "exited")
			&& matches!(
				kind.as_str(),
				"commandExecution" | "fileChange" | "mcpToolCall" | "dynamicToolCall" | "webSearch"
			) =>
			Some(turn_id),
		_ => None,
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
			Instant, IntoElement, ItemKey, MAX_STAGGER, ParentElement, Rc, Row, STAGGER, Styled,
			Window,
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

				attachments: vec![],
			},
		}
	}

	#[test]
	fn one_append_coalesces_tools_but_preserves_running_failed_and_later_rows() {
		fn tool(position: u64, status: &str) -> AgentTimelineEntry {
			let mut entry = row(position);
			if let AgentTimelineContent::Item { activity, .. } = &mut entry.content {
				*activity = Some(
					serde_json::from_value(serde_json::json!({
						"turn_id": "turn", "item_id": position.to_string(), "kind": "dynamicToolCall",
						"status": status, "label": "Tool", "detail": "", "duration_ms": null
					}))
					.unwrap(),
				);
			}
			entry
		}
		let mut arrivals = Arrivals::default();
		let mut entries = vec![row(1)];
		arrivals.observe(&entries, true);
		entries.extend((2..=7).map(|i| tool(i, "completed")));
		entries.extend([tool(8, "failed"), tool(9, "running")]);
		arrivals.observe(&entries, true);
		assert_eq!(arrivals.tool_batch(&entries[1..]).unwrap().count, 6);
		assert!(arrivals.active(&entries[1]));
		assert!(entries[2..7].iter().all(|e| !arrivals.active(e)));
		assert!(entries[7..].iter().all(|e| arrivals.active(e)));
		arrivals.expand_tool_batch(&ItemKey::for_entry(&entries[1]).unwrap(), true);
		entries.push(tool(10, "completed"));
		arrivals.observe(&entries, false);
		let batch = arrivals.tool_batch(&entries[1..]).unwrap();
		assert_eq!(batch.count, 6);
		assert!(batch.expanded);
		assert!(arrivals.tool_batch(&entries[9..]).is_none());
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
	fn repeated_item_ids_do_not_share_arrival_or_live_promotion_state() {
		let mut arrivals = Arrivals::default();
		let old = row(1);
		arrivals.observe(std::slice::from_ref(&old), true);
		let mut live = row(2);
		if let AgentTimelineContent::Item { turn_id, item_id, .. } = &mut live.content {
			*turn_id = "next-turn".into();
			*item_id = "1".into();
		}
		arrivals.present_live("next-turn", "1");
		arrivals.observe(&[old.clone(), live.clone()], true);
		assert!(!arrivals.active(&old));
		assert!(!arrivals.active(&live), "a displayed live row must not replay its arrival");
		let mut next = live.clone();
		next.position = 3;
		if let AgentTimelineContent::Item { turn_id, .. } = &mut next.content {
			*turn_id = "third-turn".into();
		}
		arrivals.observe(&[old.clone(), live.clone(), next.clone()], true);
		assert!(arrivals.active(&next));
		assert!(!arrivals.active(&old), "the older row must stay at its settled height");
		assert!(!arrivals.active(&live));
	}

	#[test]
	fn a_tool_burst_has_a_short_bounded_stagger_and_no_initial_velocity_jump() {
		let mut arrivals = Arrivals::default();

		arrivals.observe(&[row(1)], true);
		arrivals.observe(&(1..=8).map(row).collect::<Vec<_>>(), true);

		let first = arrivals.rows[&ItemKey::new("turn", "2")].started;

		assert_eq!(arrivals.rows[&ItemKey::new("turn", "3")].started - first, STAGGER);
		assert_eq!(arrivals.rows[&ItemKey::new("turn", "8")].started - first, MAX_STAGGER);

		arrivals.observe(&(1..=8).map(row).collect::<Vec<_>>(), true);

		assert_eq!(arrivals.rows[&ItemKey::new("turn", "3")].started - first, STAGGER);
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

		let born = arrivals.rows[&ItemKey::new("turn", "12")].started;

		arrivals.observe(&[row(11), row(12)], true);

		assert_eq!(arrivals.rows[&ItemKey::new("turn", "12")].started, born);

		arrivals.observe(&[row(12), row(13)], false);

		assert!(
			!arrivals.rows.contains_key(&ItemKey::new("turn", "13")),
			"background updates stay static"
		);
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
