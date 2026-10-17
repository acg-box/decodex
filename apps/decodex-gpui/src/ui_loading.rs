//! Compact feedback for a first read. Refreshes with retained content stay quiet.
use std::{f32::consts::TAU, time::Instant};

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
		.text_color(gpui::rgb(TEXT_MUTED))
		.child(gpui::div().flex().items_center().gap(gpui::px(4. * scale)).children((0..3).map(
			|i| {
				gpui::div()
					.size(gpui::px(4. * scale))
					.rounded_full()
					.bg(gpui::rgb(TEXT_MUTED))
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
					.text_color(gpui::rgb(TEXT_MUTED))
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
}
#[derive(Default)]
struct Arrival {
	loaded: bool,
	started: Option<Instant>,
	geometry:
		Option<(gpui::Bounds<gpui::Pixels>, gpui::Point<gpui::Pixels>, gpui::Point<gpui::Pixels>)>,
	stable_frames: u8,
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
}
impl RenderOnce for Content {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("load-arrival-{}", self.key)),
			cx,
			|_, _| Arrival::default(),
		);
		let settled = self.layout.as_ref().is_none_or(|(settled, _)| *settled);
		let reduced = ui_motion::reduced();
		let opacity = state.update(cx, |s, _| {
			if self.ready && !s.loaded && s.started.is_none() {
				s.geometry = None;
				s.stable_frames = 0;
			}
			s.loaded = self.ready;
			if self.ready
				&& s.started.is_none()
				&& self.layout.is_some()
				&& (!settled || (!reduced && s.stable_frames < 2))
			{
				0.
			} else {
				s.progress(self.ready, Instant::now(), reduced)
			}
		});
		if opacity < 1. && settled {
			ui_motion::request_frame(window, cx);
		}
		let mut body =
			gpui::div().w_full().min_w_0().flex_none().opacity(opacity).child(self.child);
		if let Some((_, scroll)) = self.layout {
			body = body.on_children_prepainted(move |bounds, _, cx| {
				if state.read(cx).started.is_some() {
					return;
				}
				let Some(bounds) = bounds.first().copied() else {
					return;
				};
				let geometry = (bounds, scroll.offset(), scroll.max_offset());
				if state.read(cx).geometry != Some(geometry) || state.read(cx).stable_frames < 2 {
					state.update(cx, |s, _| s.observe_layout(geometry));
				}
			});
		}
		body
	}
}
pub(crate) fn content(
	key: impl Into<SharedString>,
	ready: bool,
	child: impl IntoElement,
) -> Content {
	Content { key: key.into(), ready, child: child.into_any_element(), layout: None }
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
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
}
