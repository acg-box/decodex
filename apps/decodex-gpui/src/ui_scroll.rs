//! Display-paced wheel motion. Precise trackpad deltas retain native momentum.
use std::time::Instant;

use gpui::{
	App, Div, ElementId, IntoElement, RenderOnce, ScrollDelta, ScrollHandle, Stateful, Window,
	prelude::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _},
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
		let state = window.use_keyed_state(self.id.clone(), cx, |_, _| AreaState::default());
		let owner = window.current_view();
		let scroll = state.update(cx, |s, cx| {
			if let Some(motion) = &s.motion {
				let (offset, moving) =
					if enabled() { motion.sample(Instant::now()) } else { (motion.to, false) };

				s.scroll.set_offset(gpui::point(
					gpui::px(0.),
					gpui::px(offset.clamp(-f32::from(s.scroll.max_offset().y).max(0.), 0.)),
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

		let wheel_state = state.clone();
		let content = self.content.overflow_hidden().track_scroll(&scroll).on_scroll_wheel(
			move |event, _window, cx| {
				let delta = event.delta.pixel_delta(gpui::px(BODY_LINE_HEIGHT)).y;

				if delta == gpui::px(0.) {
					return;
				}

				wheel_state.update(cx, |s, _| {
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

						s.scroll.set_offset(gpui::point(
							gpui::px(0.),
							gpui::px((current + f32::from(delta)).clamp(-maximum, 0.)),
						));
					}
				});

				cx.stop_propagation();
				cx.notify(owner);
			},
		);
		content.into_any_element()
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
	use std::time::{Duration, Instant};

	use gpui::{self, ScrollDelta};

	use crate::ui_scroll::{self, Motion};

	#[test]
	fn disabling_motion_is_immediate_and_precise_gestures_are_never_resmoothed() {
		assert!(ui_scroll::should_smooth(ScrollDelta::Lines(gpui::point(0., -1.)), true));
		assert!(!ui_scroll::should_smooth(ScrollDelta::Lines(gpui::point(0., -1.)), false));
		assert!(!ui_scroll::should_smooth(
			ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-0.25))),
			true
		));
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

/// Overlay control: never changes text width or owns a second scroll position.
#[derive(IntoElement)]
pub(crate) struct Scrollbar {
	pub id: ElementId,
	pub scroll: ScrollHandle,
	pub changed: std::rc::Rc<dyn Fn(f32, &mut Window, &mut App)>,
}
#[derive(Default)]
struct BarState {
	grab: Option<f32>,
	hovered: bool,
}
impl RenderOnce for Scrollbar {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| BarState::default());
		let owner = window.current_view();
		let hit_scroll = self.scroll.clone();
		gpui::canvas(
			move |bounds, window, _| {
				(hit_scroll.max_offset().y > gpui::px(0.))
					.then(|| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal))
			},
			move |bounds, hitbox, window, cx| {
				let Some(hitbox) = hitbox else {
					return;
				};
				let maximum = f32::from(self.scroll.max_offset().y).max(0.);
				let height = f32::from(bounds.size.height);
				let viewport = f32::from(self.scroll.bounds().size.height);
				if maximum <= 0. || height <= 0. || viewport <= 0. {
					return;
				}
				let thumb = (height * viewport / (viewport + maximum)).max(28.).min(height);
				let travel = height - thumb;
				if travel <= 0. {
					return;
				}
				let top = -f32::from(self.scroll.offset().y).clamp(-maximum, 0.) / maximum * travel;
				let active = state.read(cx).hovered || state.read(cx).grab.is_some();
				let width = if active { 6. } else { 4. };
				let rect = gpui::Bounds::new(
					gpui::point(
						bounds.origin.x + gpui::px((12. - width) / 2.),
						bounds.origin.y + gpui::px(top),
					),
					gpui::size(gpui::px(width), gpui::px(thumb)),
				);
				window.paint_quad(
					gpui::fill(rect, gpui::rgba(if active { 0xffffff70 } else { 0xffffff30 }))
						.corner_radii(gpui::px(3.)),
				);
				let down_hitbox = hitbox.clone();
				let down_state = state.clone();
				let down_changed = self.changed.clone();
				window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
					if !phase.bubble()
						|| event.button != gpui::MouseButton::Left
						|| !down_hitbox.is_hovered(window)
					{
						return;
					}
					let y = f32::from(event.position.y - bounds.origin.y);
					let grab = if y >= top && y <= top + thumb { y - top } else { thumb / 2. };
					down_state.update(cx, |s, _| s.grab = Some(grab));
					down_changed(-((y - grab) / travel).clamp(0., 1.) * maximum, window, cx);
					cx.stop_propagation();
					cx.notify(owner);
				});
				let move_state = state.clone();
				let changed = self.changed.clone();
				window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
					if !phase.bubble() {
						return;
					}
					let hovered = hitbox.is_hovered(window);
					if move_state.read(cx).hovered != hovered {
						move_state.update(cx, |s, _| s.hovered = hovered);
						cx.notify(owner);
					}
					if let Some(grab) = move_state.read(cx).grab {
						if event.pressed_button != Some(gpui::MouseButton::Left) {
							move_state.update(cx, |s, _| s.grab = None);
							return;
						}
						let y = f32::from(event.position.y - bounds.origin.y);
						changed(-((y - grab) / travel).clamp(0., 1.) * maximum, window, cx);
						cx.stop_propagation();
						cx.notify(owner);
					}
				});
				window.on_mouse_event(move |event: &gpui::MouseUpEvent, _, _, cx| {
					if event.button == gpui::MouseButton::Left && state.read(cx).grab.is_some() {
						state.update(cx, |s, _| s.grab = None);
						cx.notify(owner);
					}
				});
			},
		)
		.absolute()
		.right(gpui::px(2.))
		.top(gpui::px(4.))
		.bottom(gpui::px(4.))
		.w(gpui::px(12.))
	}
}

#[cfg(test)]
mod scrollbar_tests {
	use super::Scrollbar;
	use gpui::{
		Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render,
		ScrollHandle, StatefulInteractiveElement as _, Styled as _, TestAppContext, Window, div,
		point, px, size,
	};

	struct Fixture {
		scroll: ScrollHandle,
	}
	impl Render for Fixture {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			let scroll = self.scroll.clone();
			let owner = cx.entity();
			div()
				.size_full()
				.relative()
				.child(
					div()
						.id("scroll-content")
						.debug_selector(|| "scroll-content".into())
						.size_full()
						.overflow_hidden()
						.track_scroll(&scroll)
						.child(div().h(px(2000.)).w_full().flex_none()),
				)
				.child(Scrollbar {
					id: "test-scrollbar".into(),
					scroll: scroll.clone(),
					changed: std::rc::Rc::new(move |offset, _, cx| {
						scroll.set_offset(point(px(0.), px(offset)));
						owner.update(cx, |_, cx| cx.notify());
					}),
				})
		}
	}
	#[gpui::test]
	fn drag_continues_outside_the_thumb_and_release_stops_scrolling(cx: &mut TestAppContext) {
		let (view, visual) = cx.add_window_view(|_, _| Fixture { scroll: ScrollHandle::new() });
		visual.simulate_resize(size(px(400.), px(300.)));
		visual.update(|window, cx| window.draw(cx).clear());
		let bounds = visual.debug_bounds("scroll-content").unwrap();
		let start = point(bounds.right() - px(8.), bounds.top() + px(8.));
		visual.simulate_mouse_move(start, None, Default::default());
		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		let end = point(bounds.center().x, bounds.bottom() - px(1.));
		visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
		visual.update(|window, cx| window.draw(cx).clear());
		let scroll = view.read_with(visual, |s, _| s.scroll.clone());
		assert!(scroll.max_offset().y > px(1000.));
		assert!((scroll.offset().y + scroll.max_offset().y).abs() < px(1.));
		visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
		let stopped = scroll.offset();
		visual.simulate_mouse_move(start, None, Default::default());
		assert_eq!(scroll.offset(), stopped);
	}
}
