//! Display-paced wheel motion. Precise trackpad deltas retain native momentum.
use std::time::Instant;

use gpui::{
	App, Div, ElementId, IntoElement, RenderOnce, ScrollDelta, ScrollHandle, Stateful, Window,
	point, prelude::*, px,
};

use crate::{ui_motion, ui_preferences, ui_theme::BODY_LINE_HEIGHT};

pub(crate) trait SmoothScrollArea {
	fn smooth_scroll(self, id: impl Into<ElementId>) -> ScrollArea;
}

/// Critically damped motion preserves velocity across consecutive wheel notches.
/// Absolute time keeps travel identical at 60 Hz and 120 Hz; no idle timer runs.
pub(crate) struct Motion {
	pub from: f32,
	pub to: f32,
	pub started: Instant,
	velocity: f32,
}
impl Motion {
	pub fn new(offset: f32, now: Instant) -> Self {
		Self { from: offset, to: offset, started: now, velocity: 0. }
	}

	fn position_velocity(&self, now: Instant) -> (f32, f32) {
		let t = now.saturating_duration_since(self.started).as_secs_f32();
		let distance = self.from - self.to;
		let decay = (-32. * t).exp();
		let c = self.velocity + 32. * distance;

		(self.to + (distance + c * t) * decay, (self.velocity - 32. * c * t) * decay)
	}

	pub fn sample(&self, now: Instant) -> (f32, bool) {
		let (offset, velocity) = self.position_velocity(now);

		if (offset - self.to).abs() < 0.1 && velocity.abs() < 3. {
			(self.to, false)
		} else {
			(offset, true)
		}
	}

	pub fn retarget(&mut self, current: f32, delta: f32, maximum: f32, now: Instant) {
		let continuing = (self.to - current) * delta > 0.;

		self.velocity = if continuing { self.position_velocity(now).1 } else { 0. };

		let base = if continuing { self.to } else { current };

		self.from = current;
		self.to = (base + delta).clamp(-maximum.max(0.), 0.);
		// A newly reached boundary must not inherit enough velocity to overshoot.
		let limit = 32. * (self.to - self.from).abs();

		self.velocity = self.velocity.clamp(-limit, limit);
		self.started = now;
	}
}

/// A regular vertical scroll area shares the transcript's motion policy without
/// owning transcript-specific pagination, follow mode, or navigation anchors.
#[derive(IntoElement)]
pub(crate) struct ScrollArea {
	id: ElementId,
	content: Stateful<Div>,
}
impl RenderOnce for ScrollArea {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| AreaState::default());
		let owner = window.current_view();
		let scroll = state.update(cx, |s, cx| {
			if let Some(motion) = &s.motion {
				let (offset, moving) =
					if enabled() { motion.sample(Instant::now()) } else { (motion.to, false) };

				s.scroll.set_offset(point(
					px(0.),
					px(offset.clamp(-f32::from(s.scroll.max_offset().y).max(0.), 0.)),
				));

				if moving {
					ui_motion::request_frame(window, cx);

					cx.notify();
				} else {
					s.motion = None;
				}
			}

			s.scroll.clone()
		});

		self.content.overflow_hidden().track_scroll(&scroll).on_scroll_wheel(
			move |event, _window, cx| {
				let delta = event.delta.pixel_delta(px(BODY_LINE_HEIGHT)).y;

				if delta == px(0.) {
					return;
				}

				state.update(cx, |s, _| {
					let current = f32::from(s.scroll.offset().y);
					let maximum = f32::from(s.scroll.max_offset().y).max(0.);

					if smooth(event.delta) {
						let now = Instant::now();

						s.motion.get_or_insert_with(|| Motion::new(current, now)).retarget(
							current,
							delta.into(),
							maximum,
							now,
						);
					} else {
						s.motion = None;

						s.scroll.set_offset(point(
							px(0.),
							px((current + f32::from(delta)).clamp(-maximum, 0.)),
						));
					}
				});

				cx.stop_propagation();
				cx.notify(owner);
			},
		)
	}
}

#[derive(Default)]
struct AreaState {
	scroll: ScrollHandle,
	motion: Option<Motion>,
}

impl SmoothScrollArea for Stateful<Div> {
	fn smooth_scroll(self, id: impl Into<ElementId>) -> ScrollArea {
		ScrollArea { id: id.into(), content: self }
	}
}

pub(crate) fn preference(value: Option<bool>) -> bool {
	static VALUE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(u8::MAX);

	ui_preferences::boolean("DecodexSmoothScrolling", &VALUE, value, true)
}

pub(crate) fn enabled() -> bool {
	preference(None) && !ui_motion::reduced()
}

pub(crate) fn smooth(delta: ScrollDelta) -> bool {
	should_smooth(delta, enabled())
}

fn should_smooth(delta: ScrollDelta, enabled: bool) -> bool {
	enabled && matches!(delta, ScrollDelta::Lines(_))
}

#[cfg(test)]
mod tests {
	use crate::ui_scroll::*;

	use std::time::Duration;

	#[test]
	fn disabling_motion_is_immediate_and_precise_gestures_are_never_resmoothed() {
		assert!(should_smooth(ScrollDelta::Lines(point(0., -1.)), true));
		assert!(!should_smooth(ScrollDelta::Lines(point(0., -1.)), false));
		assert!(!should_smooth(ScrollDelta::Pixels(point(px(0.), px(-0.25))), true));
	}

	#[test]
	fn successive_notches_preserve_velocity_and_reversal_cancels_it() {
		let now = Instant::now();
		let mut motion = Motion::new(-100., now);

		motion.retarget(-100., -50., 1_000., now);

		let later = now + Duration::from_millis(40);
		let (position, velocity) = motion.position_velocity(later);

		motion.retarget(position, -50., 1_000., later);

		assert_eq!(motion.to, -200.);
		assert_eq!(motion.position_velocity(later), (position, velocity));

		motion.retarget(position, 20., 1_000., later);

		assert_eq!(motion.to, position + 20.);
		assert_eq!(motion.position_velocity(later).1, 0.);
	}

	#[test]
	fn motion_is_monotonic_frame_independent_and_eventually_stops() {
		let now = Instant::now();
		let mut motion = Motion::new(0., now);

		motion.retarget(0., -200., 100., now);

		for hz in [60, 120, 144] {
			let mut previous = 0.;

			for frame in 1..=hz {
				let (position, _) =
					motion.sample(now + Duration::from_secs_f32(frame as f32 / hz as f32));

				assert!(position <= previous && position >= -100.);

				previous = position;
			}

			assert_eq!(previous, -100.);
		}

		assert_eq!(motion.sample(now + Duration::from_secs(1)), (-100., false));
	}
}
