//! Compact feedback for a first read. Refreshes with retained content stay quiet.
use std::{
	cell::RefCell,
	f32::consts::TAU,
	rc::Rc,
	time::{Duration, Instant},
};

use gpui::{
	self, AnyElement, App, IntoElement, RenderOnce, Role, SharedString, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{ui_motion, ui_theme::TEXT_MUTED};

#[derive(IntoElement)]
pub(crate) struct Loading {
	label: &'static str,
	icon_only: bool,
}
impl RenderOnce for Loading {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let clock = window.use_keyed_state(
			SharedString::from(format!("loading-{}", self.label)),
			cx,
			|_, _| Instant::now(),
		);
		let elapsed = clock.read(cx).elapsed().as_secs_f32();
		let reduced = ui_motion::reduced();

		if !reduced {
			ui_motion::request_frame(window, cx);
		}

		indicator(self.label, self.icon_only, elapsed, reduced).opacity(if reduced {
			1.
		} else {
			((elapsed - 0.12) / 0.12).clamp(0., 1.)
		})
	}
}

fn indicator(
	label: &'static str,
	icon_only: bool,
	elapsed: f32,
	reduced: bool,
) -> gpui::Stateful<gpui::Div> {
	let scale = if icon_only { 1.5 } else { 1. };
	let phase = if reduced { 0. } else { elapsed * TAU / 1.4 };

	gpui::div()
		.id(SharedString::from(format!("loading-row-{label}")))
		.role(Role::Status)
		.aria_label(if label.is_empty() { "Loading" } else { label })
		.debug_selector(move || format!("loading-feedback-{label}"))
		.h(gpui::px(24.))
		.flex_none()
		.flex()
		.items_center()
		.gap(gpui::px(8.))
		.text_size(gpui::px(11.))
		.text_color(crate::ui_theme::rgb(TEXT_MUTED))
		.child(gpui::div().flex().items_center().gap(gpui::px(4. * scale)).children((0..3).map(
			|i| {
				gpui::div()
					.size(gpui::px(4. * scale))
					.rounded_full()
					.bg(crate::ui_theme::rgb(TEXT_MUTED))
					.opacity(if reduced {
						0.6
					} else {
						0.3 + 0.45 * ((phase - i as f32 * 0.7).sin() + 1.) / 2.
					})
			},
		)))
		.when(!icon_only && !label.is_empty(), |row| row.child(label))
}

/// First-load placeholder that reserves the reading surface, not a toolbar row.
#[derive(IntoElement)]
pub(crate) struct ConversationLoading {
	label: &'static str,
}
impl RenderOnce for ConversationLoading {
	fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
		// The history shape is unknown until read. Do not invent bubbles or text lengths.
		gpui::div()
			.id(SharedString::from(format!("conversation-loading-surface-{}", self.label)))
			.role(Role::Status)
			.aria_label(self.label)
			.debug_selector(move || format!("loading-feedback-{}", self.label))
			.w_full()
			.min_h(gpui::px(260.))
			.flex()
			.items_center()
			.justify_center()
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap(gpui::px(8.))
					.text_size(gpui::px(12.))
					.text_color(crate::ui_theme::rgb(TEXT_MUTED))
					.child(loading(""))
					.child(self.label),
			)
	}
}

pub(crate) fn loading(label: &'static str) -> Loading {
	Loading { label, icon_only: false }
}

/// Keep mounted across page requests so brief completions do not restart the indicator.
#[derive(IntoElement)]
pub(crate) struct PaginationLoading {
	active: bool,
}
impl RenderOnce for PaginationLoading {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state =
			window.use_keyed_state("pagination-loading", cx, |_, _| PaginationPresence::default());
		let reduced = ui_motion::reduced();
		let now = Instant::now();
		let (opacity, elapsed) = state.update(cx, |s, _| {
			(s.opacity(self.active, now), now.duration_since(s.started).as_secs_f32())
		});

		if (self.active && (!reduced || opacity == 0.)) || (!self.active && opacity > 0.) {
			ui_motion::request_frame(window, cx);
		}

		gpui::div().when(opacity > 0., |row| {
			row.child(indicator("Loading earlier history", true, elapsed, reduced).opacity(opacity))
		})
	}
}

pub(crate) fn pagination(active: bool) -> PaginationLoading {
	PaginationLoading { active }
}

struct PaginationPresence {
	active: bool,
	started: Instant,
	fade: ui_motion::Tween,
}
impl Default for PaginationPresence {
	fn default() -> Self {
		Self { active: false, started: Instant::now(), fade: ui_motion::Tween::new(0.) }
	}
}
impl PaginationPresence {
	fn opacity(&mut self, active: bool, now: Instant) -> f32 {
		if active && !self.active && self.fade.sample(now) == 0. {
			self.started = now;
		}
		self.active = active;

		let visible = active && now.duration_since(self.started).as_millis() >= 250;
		self.fade.target(if visible { 1. } else { 0. }, now);
		self.fade.sample(now)
	}
}

pub(crate) fn conversation(label: &'static str) -> ConversationLoading {
	ConversationLoading { label }
}

/// Quiet full-workspace feedback; the status remains available to assistive technology.
pub(crate) fn workspace() -> Loading {
	Loading { label: "Opening workspace", icon_only: true }
}

/// One clock for the first workspace reveal, including the native composer window.
#[derive(Default)]
pub(crate) struct WorkspaceArrival {
	started: Option<Instant>,
}
impl WorkspaceArrival {
	pub(crate) fn observe(&mut self, ready: bool) {
		if ready {
			self.started.get_or_insert_with(Instant::now);
		} else {
			self.started = None;
		}
	}

	pub(crate) fn progress(&self, delay: f32) -> f32 {
		self.progress_at(Instant::now(), delay, ui_motion::reduced())
	}

	fn progress_at(&self, now: Instant, delay: f32, reduced: bool) -> f32 {
		let Some(started) = self.started else {
			return 0.;
		};
		if reduced {
			return 1.;
		}
		let t =
			((now.saturating_duration_since(started).as_secs_f32() - delay) / 0.48).clamp(0., 1.);
		t * t * (3. - 2. * t)
	}
}

/// Fade each new load. Refreshes that retain ready content do not replay it.
#[derive(IntoElement)]
pub(crate) struct Content {
	key: SharedString,
	ready: bool,
	child: AnyElement,
	layout: Option<(bool, gpui::ScrollHandle)>,
	arrival: Option<ContentArrival>,
}

/// Keep a conversation's reveal clock with its cached history across navigation.
#[derive(Clone, Default)]
pub(crate) struct ContentArrival(Rc<RefCell<Arrival>>);

impl Content {
	pub(crate) fn with_arrival(mut self, arrival: ContentArrival) -> Self {
		self.arrival = Some(arrival);
		self
	}
}

#[derive(Default)]
struct Arrival {
	loaded: bool,
	started: Option<Instant>,
	geometry:
		Option<(gpui::Bounds<gpui::Pixels>, gpui::Point<gpui::Pixels>, gpui::Point<gpui::Pixels>)>,
	stable_frames: u8,
	pending_since: Option<Instant>,
	loader_at_reveal: f32,
}

const HISTORY_FEEDBACK_DELAY: Duration = Duration::from_millis(250);
const HISTORY_FEEDBACK_FADE: f32 = 0.12;
const HISTORY_HANDOFF: f32 = 0.16;

struct HistoryFrame {
	content: f32,
	feedback: f32,
	elapsed: f32,
	pending: bool,
}
impl Arrival {
	fn observe_layout(
		&mut self,
		geometry: (
			gpui::Bounds<gpui::Pixels>,
			gpui::Point<gpui::Pixels>,
			gpui::Point<gpui::Pixels>,
		),
	) {
		if self.geometry == Some(geometry) {
			self.stable_frames = (self.stable_frames + 1).min(2);
		} else {
			self.geometry = Some(geometry);
			self.stable_frames = 0;
		}
	}

	fn progress(&mut self, ready: bool, now: Instant, reduced: bool) -> f32 {
		if !ready {
			// Retained refreshes stay ready; a cleared surface starts a new arrival.
			self.started = None;
			return 1.;
		}
		let started = *self.started.get_or_insert(now);
		if reduced {
			return 1.;
		}
		let t = (now.saturating_duration_since(started).as_secs_f32() / 0.32).clamp(0., 1.);
		t * t * (3. - 2. * t)
	}

	fn feedback(&self, now: Instant, reduced: bool) -> f32 {
		let Some(since) = self.pending_since else { return 0. };
		let elapsed = now.saturating_duration_since(since);
		if elapsed < HISTORY_FEEDBACK_DELAY {
			return 0.;
		}
		if reduced {
			return 1.;
		}
		let t = ((elapsed - HISTORY_FEEDBACK_DELAY).as_secs_f32() / HISTORY_FEEDBACK_FADE).min(1.);
		t * t * (3. - 2. * t)
	}

	/// Fast reads reveal immediately once layout settles. A visible loader and
	/// incoming content share one handoff; content never waits for a minimum dwell.
	fn history_frame(
		&mut self,
		ready: bool,
		settled: bool,
		now: Instant,
		reduced: bool,
	) -> HistoryFrame {
		if !ready && self.started.take().is_some() {
			self.pending_since = None;
			self.loader_at_reveal = 0.;
		}
		let pending = !ready
			|| (self.started.is_none() && (!settled || (!reduced && self.stable_frames < 2)));
		if pending {
			let since = *self.pending_since.get_or_insert(now);
			self.loader_at_reveal = self.feedback(now, reduced);
			return HistoryFrame {
				content: 0.,
				feedback: self.loader_at_reveal,
				elapsed: now.saturating_duration_since(since).as_secs_f32(),
				pending: true,
			};
		}
		// Use what was actually shown, not a delayed frame's theoretical opacity.
		let started = *self.started.get_or_insert(now);
		let t = if reduced || self.loader_at_reveal == 0. {
			1.
		} else {
			(now.saturating_duration_since(started).as_secs_f32() / HISTORY_HANDOFF).min(1.)
		};
		let feedback = self.loader_at_reveal * (1. - t * t * (3. - 2. * t));
		HistoryFrame {
			content: 1. - feedback,
			feedback,
			elapsed: self
				.pending_since
				.map_or(0., |since| now.saturating_duration_since(since).as_secs_f32()),
			pending: false,
		}
	}
}
impl RenderOnce for Content {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = self.arrival.unwrap_or_else(|| {
			window
				.use_keyed_state(
					SharedString::from(format!("load-arrival-{}", self.key)),
					cx,
					|_, _| ContentArrival::default(),
				)
				.read(cx)
				.clone()
		});
		let settled = self.layout.as_ref().is_none_or(|(settled, _)| *settled);
		let history = self.layout.is_some();
		let feedback_top = self
			.layout
			.as_ref()
			.map_or(gpui::px(0.), |(_, scroll)| -scroll.offset().y)
			.max(gpui::px(0.));
		let reduced = ui_motion::reduced();
		let (opacity, feedback) = {
			let mut s = state.0.borrow_mut();
			if self.ready && !s.loaded && s.started.is_none() {
				s.geometry = None;
				s.stable_frames = 0;
			}
			s.loaded = self.ready;
			if history {
				let frame = s.history_frame(self.ready, settled, Instant::now(), reduced);
				(frame.content, Some(frame))
			} else {
				(s.progress(self.ready, Instant::now(), reduced), None)
			}
		};
		if feedback.as_ref().is_some_and(|f| {
			(f.pending && (!reduced || f.feedback == 0.)) || (!f.pending && f.content < 1.)
		}) || (!history && opacity < 1. && settled)
		{
			ui_motion::request_frame(window, cx);
		}
		let mut body =
			gpui::div().w_full().min_w_0().flex_none().opacity(opacity).child(self.child);
		if let Some((_, scroll)) = self.layout {
			body = body.on_children_prepainted(move |bounds, window, cx| {
				let mut state = state.0.borrow_mut();
				if state.started.is_some() {
					return;
				}
				let Some(bounds) = bounds.first().copied() else {
					return;
				};
				let geometry = (bounds, scroll.offset(), scroll.max_offset());
				if state.geometry != Some(geometry) || state.stable_frames < 2 {
					state.observe_layout(geometry);
					ui_motion::request_frame(window, cx);
				}
			});
		}
		if let Some(feedback) = feedback {
			gpui::div()
				.relative()
				.w_full()
				.min_w_0()
				.flex_none()
				.when(!self.ready, |d| d.min_h(gpui::px(260.)))
				.child(body)
				.when(feedback.feedback > 0., |d| {
					d.child(
						gpui::div()
							.absolute()
							.top(feedback_top)
							.left_0()
							.w_full()
							.h(gpui::px(260.))
							.flex()
							.items_center()
							.justify_center()
							.opacity(feedback.feedback)
							.child(indicator(
								"Loading conversation",
								false,
								feedback.elapsed,
								reduced,
							)),
					)
				})
		} else {
			body
		}
	}
}
pub(crate) fn content(
	key: impl Into<SharedString>,
	ready: bool,
	child: impl IntoElement,
) -> Content {
	Content { key: key.into(), ready, child: child.into_any_element(), layout: None, arrival: None }
}

/// Lay out the initial history and restore its scroll anchor before revealing it.
pub(crate) fn history(
	key: impl Into<SharedString>,
	ready: bool,
	settled: bool,
	scroll: gpui::ScrollHandle,
	child: impl IntoElement,
) -> Content {
	Content {
		key: key.into(),
		ready,
		child: child.into_any_element(),
		layout: Some((settled, scroll)),
		arrival: None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fast_history_reads_never_flash_feedback_or_wait_for_an_arrival_animation() {
		let start = Instant::now();
		let mut state = Arrival::default();
		let waiting = state.history_frame(false, false, start, false);
		assert_eq!((waiting.content, waiting.feedback), (0., 0.));
		state.stable_frames = 2;
		let ready = state.history_frame(true, true, start + Duration::from_millis(180), false);
		assert_eq!((ready.content, ready.feedback), (1., 0.));
		let cached = state.history_frame(true, false, start + Duration::from_secs(1), false);
		assert_eq!(
			(cached.content, cached.feedback),
			(1., 0.),
			"cached history does not replay loading after navigation"
		);
		let mut delayed_frame = Arrival::default();
		delayed_frame.history_frame(false, false, start, false);
		delayed_frame.stable_frames = 2;
		let ready = delayed_frame.history_frame(true, true, start + Duration::from_secs(1), false);
		assert_eq!(
			(ready.content, ready.feedback),
			(1., 0.),
			"a late frame must not introduce feedback that was never shown"
		);
	}

	#[test]
	fn slow_history_crossfades_without_holding_back_the_result() {
		let start = Instant::now();
		let at = |ms| start + Duration::from_millis(ms);
		let mut state = Arrival::default();
		state.history_frame(false, false, start, false);
		assert_eq!(state.history_frame(false, false, at(249), false).feedback, 0.);
		assert_eq!(state.history_frame(false, false, at(370), false).feedback, 1.);
		state.stable_frames = 2;
		let ready = state.history_frame(true, true, at(400), false);
		assert_eq!((ready.content, ready.feedback), (0., 1.));
		let during = state.history_frame(true, true, at(480), false);
		assert!((during.content - 0.5).abs() < 0.001);
		assert!((during.content + during.feedback - 1.).abs() < 0.001);
		let done = state.history_frame(true, true, at(560), false);
		assert_eq!((done.content, done.feedback), (1., 0.));
		let next = state.history_frame(false, false, at(600), false);
		assert_eq!((next.content, next.feedback), (0., 0.), "new reads get their own delay");
	}

	#[test]
	fn feedback_cannot_flash_off_at_the_delay_boundary_and_reduced_motion_stays_delayed() {
		let start = Instant::now();
		let at = |ms| start + Duration::from_millis(ms);
		let mut state = Arrival::default();
		state.history_frame(false, false, start, false);
		let opening = state.history_frame(false, false, at(280), false).feedback;
		state.stable_frames = 2;
		let ready = state.history_frame(true, true, at(280), false);
		assert_eq!(ready.feedback, opening, "the handoff starts at the displayed opacity");
		assert_eq!(ready.content, 1. - opening);
		let during = state.history_frame(true, true, at(300), false);
		assert!(during.feedback < opening && during.feedback > 0. && during.content > 0.);
		let mut reduced = Arrival::default();
		assert_eq!(reduced.history_frame(false, false, start, true).feedback, 0.);
		assert_eq!(reduced.history_frame(false, false, at(249), true).feedback, 0.);
		assert_eq!(reduced.history_frame(false, false, at(250), true).feedback, 1.);
		assert_eq!(reduced.history_frame(true, true, at(251), true).content, 1.);
	}

	#[test]
	fn pagination_feedback_ignores_fast_reads_and_stays_continuous_between_pages() {
		let mut state = PaginationPresence::default();
		let start = Instant::now();
		let at = |ms| start + std::time::Duration::from_millis(ms);

		assert_eq!(state.opacity(true, at(0)), 0.);
		assert_eq!(state.opacity(false, at(100)), 0., "fast reads stay invisible");
		assert_eq!(state.opacity(true, at(110)), 0.);
		assert_eq!(state.opacity(true, at(350)), 0.);
		assert_eq!(state.opacity(true, at(360)), 0., "fade starts after the delay");
		assert_eq!(state.opacity(true, at(560)), 1.);
		assert_eq!(state.opacity(false, at(570)), 1.);
		let closing = state.opacity(false, at(660));
		assert!(closing > 0. && closing < 1., "completion fades instead of blinking off");
		assert_eq!(state.opacity(true, at(660)), closing, "resuming must preserve current opacity");
		assert_eq!(state.opacity(true, at(860)), 1.);
		assert_eq!(state.opacity(false, at(870)), 1.);
		assert_eq!(state.opacity(false, at(1_100)), 0.);
		assert_eq!(state.opacity(true, at(1_110)), 0., "a later read gets a new delay");
	}

	#[test]
	fn workspace_regions_reveal_in_place_and_refresh_does_not_restart_them() {
		let mut arrival = WorkspaceArrival::default();
		assert_eq!(arrival.progress(0.), 0.);
		arrival.observe(true);
		let start = arrival.started.unwrap();
		assert_eq!(arrival.progress_at(start, 0., false), 0.);
		let middle = start + std::time::Duration::from_millis(240);
		let sidebar = arrival.progress_at(middle, 0., false);
		let chat = arrival.progress_at(middle, 0.12, false);
		assert!(chat > 0. && chat < sidebar && sidebar < 1.);
		arrival.observe(true);
		assert_eq!(arrival.started, Some(start));
		assert_eq!(arrival.progress_at(start + std::time::Duration::from_secs(1), 0.12, false), 1.);
		assert_eq!(arrival.progress_at(start, 0.12, true), 1.);
		arrival.observe(false);
		assert_eq!(arrival.progress(0.), 0.);
	}
	#[test]
	fn initial_history_waits_for_scroll_and_layout_to_stop_changing() {
		let mut state = Arrival::default();
		let mut geometry =
			(gpui::Bounds::default(), gpui::Point::default(), gpui::Point::default());
		state.observe_layout(geometry);
		state.observe_layout(geometry);
		assert_eq!(state.stable_frames, 1);
		geometry.1.y = gpui::px(-400.);
		state.observe_layout(geometry);
		assert_eq!(state.stable_frames, 0, "restored scroll must settle before reveal");
		state.observe_layout(geometry);
		state.observe_layout(geometry);
		assert_eq!(state.stable_frames, 2);
		geometry.0.size.height = gpui::px(900.);
		state.observe_layout(geometry);
		assert_eq!(state.stable_frames, 0, "a layout change invalidates readiness");
	}
	#[test]
	fn retained_content_stays_visible_but_an_explicit_reload_replays_arrival() {
		let now = Instant::now();
		let mut state = Arrival::default();
		assert_eq!(state.progress(false, now, false), 1.);
		assert_eq!(state.progress(true, now, false), 0.);
		let halfway = now + std::time::Duration::from_millis(160);
		assert!((state.progress(true, halfway, false) - 0.5).abs() < 0.001);
		let done = now + std::time::Duration::from_millis(320);
		assert_eq!(state.progress(true, done, false), 1.);
		assert_eq!(state.progress(false, done, false), 1.);
		assert_eq!(state.progress(true, done, false), 0.);
		assert!(
			(state.progress(true, done + std::time::Duration::from_millis(160), false) - 0.5).abs()
				< 0.001
		);
		assert_eq!(Arrival::default().progress(true, now, true), 1.);
	}

	#[test]
	fn cached_history_keeps_its_reveal_clock_when_the_view_is_recreated() {
		let arrival = ContentArrival::default();
		let now = Instant::now();
		arrival.0.borrow_mut().stable_frames = 2;
		let first = history("thread", true, true, Default::default(), gpui::div())
			.with_arrival(arrival.clone());
		assert_eq!(
			first
				.arrival
				.as_ref()
				.unwrap()
				.0
				.borrow_mut()
				.history_frame(true, true, now, false)
				.content,
			1.
		);
		drop(first);
		let reopened =
			history("thread", true, true, Default::default(), gpui::div()).with_arrival(arrival);
		assert_eq!(
			reopened
				.arrival
				.as_ref()
				.unwrap()
				.0
				.borrow_mut()
				.history_frame(true, true, now + std::time::Duration::from_secs(1), false,)
				.content,
			1.,
		);
	}
}
