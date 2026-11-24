//! Display-paced wheel motion. Precise trackpad deltas retain native momentum.
use std::time::{Duration, Instant};

use gpui::{
	App, Bounds, DispatchPhase, Div, ElementId, Hitbox, HitboxBehavior, IntoElement, KeyDownEvent,
	MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, RenderOnce,
	Role, ScrollDelta, ScrollHandle, SharedString, Stateful, Window,
	prelude::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _},
};

use crate::{
	ui_motion, ui_preferences,
	ui_theme::{self, BODY_LINE_HEIGHT},
};

const SCROLL_RESPONSE: f32 = 40.;
const SCROLLBAR_HIDE_DELAY: Duration = Duration::from_millis(550);

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
		let decay = (-SCROLL_RESPONSE * t).exp();
		let c = self.velocity + SCROLL_RESPONSE * distance;

		(self.to + (distance + c * t) * decay, (self.velocity - SCROLL_RESPONSE * c * t) * decay)
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
		let limit = SCROLL_RESPONSE * (self.to - self.from).abs();

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
	scrollbar: Option<(&'static str, &'static str)>,
	contain: bool,
}
impl ScrollArea {
	/// Keep wheel gestures inside an overflowing nested reader, including at its ends.
	pub(crate) fn contain_scroll(mut self) -> Self {
		self.contain = true;
		self
	}

	pub(crate) fn with_scrollbar(mut self, id: &'static str, label: &'static str) -> Self {
		self.scrollbar = Some((id, label));

		self
	}
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
				} else {
					s.motion = None;
				}
			}

			s.scroll.clone()
		});
		let wheel_state = state.clone();
		let content = self.content.overflow_hidden().track_scroll(&scroll).on_scroll(
			move |event, _window, cx| {
				let delta = event.delta.pixel_delta(gpui::px(BODY_LINE_HEIGHT)).y;

				if delta == gpui::px(0.) {
					return;
				}

				let current = wheel_state.read(cx).scroll.offset().y;
				let maximum = wheel_state.read(cx).scroll.max_offset().y.max(gpui::px(0.));

				if (delta > gpui::px(0.) && current >= gpui::px(0.))
					|| (delta < gpui::px(0.) && current <= -maximum)
				{
					if self.contain && maximum > gpui::px(0.) {
						cx.stop_propagation();
					}
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

		if let Some((id, label)) = self.scrollbar {
			gpui::div()
				.relative()
				.size_full()
				.child(content)
				.child(Scrollbar::new(id, label, scroll))
				.into_any_element()
		} else {
			content.into_any_element()
		}
	}
}

#[derive(IntoElement)]
pub(crate) struct Scrollbar {
	id: &'static str,
	label: &'static str,
	scroll: ScrollHandle,
}
impl Scrollbar {
	pub(crate) fn new(id: &'static str, label: &'static str, scroll: ScrollHandle) -> Self {
		Self { id, label, scroll }
	}
}

struct ScrollbarState {
	focus: gpui::FocusHandle,
	hovered: bool,
	grab: Option<Pixels>,
	offset: Pixels,
	hide: Option<gpui::Task<()>>,
}

impl RenderOnce for Scrollbar {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let offset = self.scroll.offset().y;
		// In automatic mode, an attached mouse can select a persistent AppKit scroller.
		// Keep this overlay transient unless the user explicitly requests a visible bar.
		let auto_hide =
			crate::ui_preferences::string("AppleShowScrollBars", None).as_deref() != Some("Always");
		let state = window.use_keyed_state(self.id, cx, |_, cx| ScrollbarState {
			focus: cx.focus_handle(),
			hovered: false,
			grab: None,
			offset,
			hide: None,
		});
		let (focus, engaged, scrolling) = state.update(cx, |s, cx| {
			if std::mem::replace(&mut s.offset, offset) != offset && auto_hide {
				s.hide = Some(cx.spawn(async move |bar, cx| {
					cx.background_executor().timer(SCROLLBAR_HIDE_DELAY).await;
					let _ = bar.update(cx, |s, cx| {
						s.hide = None;
						cx.notify();
					});
				}));
			}
			let focused = window.last_input_was_keyboard() && s.focus.is_focused(window);
			(s.focus.clone(), s.hovered || s.grab.is_some() || focused, s.hide.is_some())
		});
		let target = if engaged {
			0.52
		} else if scrolling {
			0.36
		} else if auto_hide {
			0.
		} else {
			0.24
		};
		let opacity = ui_motion::direct_value(
			SharedString::from(format!("{}-opacity", self.id)),
			target,
			engaged || scrolling,
			window,
			cx,
		);
		let thumb_width = gpui::px(if engaged { 5. } else { 3. });
		let hover_state = state.clone();
		let owner = window.current_view();
		let keys = self.scroll.clone();
		let canvas = gpui::canvas(
			move |bounds, window, _| {
				scrollbar_thumb_layout(self.scroll, bounds, thumb_width, window)
			},
			move |bounds, layout, window, _| {
				let Some((scroll, thumb, travel, hitbox)) = layout else { return };

				if opacity > 0. {
					window.paint_quad(
						gpui::fill(thumb, ui_theme::rgb(ui_theme::TEXT).opacity(opacity))
							.corner_radii(thumb_width / 2.),
					);
				}

				let down_scroll = scroll.clone();
				let down_drag = state.clone();
				let down_thumb = thumb;
				let hitbox = hitbox.clone();

				window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
					if phase == DispatchPhase::Bubble
						&& event.button == MouseButton::Left
						&& hitbox.is_hovered(window)
					{
						window.prevent_default();

						let grab = if (down_thumb.top()..=down_thumb.bottom())
							.contains(&event.position.y)
						{
							event.position.y - down_thumb.top()
						} else {
							down_thumb.size.height / 2.
						};

						down_drag.update(cx, |s, _| s.grab = Some(grab));

						set_scroll(&down_scroll, event.position.y - bounds.top() - grab, travel);

						cx.notify(owner);
						cx.stop_propagation();
					}
				});

				let scroll = scroll.clone();
				let up_drag = state.clone();

				window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
					if phase == DispatchPhase::Capture {
						up_drag.update(cx, |s, cx| {
							if s.grab.take().is_some() {
								cx.notify();
							}
						});
					}
				});
				window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
					if phase != DispatchPhase::Capture {
						return;
					}

					let Some(grab) = state.read(cx).grab else { return };

					if event.pressed_button == Some(MouseButton::Left) {
						set_scroll(&scroll, event.position.y - bounds.top() - grab, travel);

						cx.notify(owner);
						cx.stop_propagation();
					} else {
						state.update(cx, |s, cx| {
							s.grab = None;
							cx.notify();
						});
					}
				});
			},
		)
		.size_full();

		gpui::div()
			.id(self.id)
			.debug_selector(move || self.id.into())
			.role(Role::ScrollBar)
			.aria_label(self.label)
			.track_focus(&focus)
			.tab_index(0)
			.on_hover(move |hovered, _, cx| {
				hover_state.update(cx, |s, cx| {
					s.hovered = *hovered;
					cx.notify();
				});
			})
			.on_key_down(move |event: &KeyDownEvent, _, cx| {
				let current = keys.offset().y;
				let page = keys.bounds().size.height;
				let maximum = keys.max_offset().y;
				let next = match event.keystroke.key.as_str() {
					"up" => current + gpui::px(40.),
					"down" => current - gpui::px(40.),
					"pageup" => current + page,
					"pagedown" => current - page,
					"home" => gpui::px(0.),
					"end" => -maximum,
					_ => return,
				};

				keys.set_offset(gpui::point(gpui::px(0.), next.clamp(-maximum, gpui::px(0.))));
				cx.notify(owner);
				cx.stop_propagation();
			})
			.absolute()
			.top(gpui::px(8.))
			.bottom(gpui::px(8.))
			.right_0()
			.w(gpui::px(14.))
			.child(canvas)
	}
}

#[derive(Default)]
struct AreaState {
	scroll: ScrollHandle,
	motion: Option<Motion>,
}

impl SmoothScrollArea for Stateful<Div> {
	fn smooth_scroll(self, id: impl Into<ElementId>) -> ScrollArea {
		ScrollArea { id: id.into(), content: self, scrollbar: None, contain: false }
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

fn scrollbar_thumb_layout(
	scroll: ScrollHandle,
	bounds: Bounds<Pixels>,
	thumb_width: Pixels,
	window: &mut Window,
) -> Option<(ScrollHandle, Bounds<Pixels>, Pixels, Hitbox)> {
	let viewport = scroll.bounds().size.height;
	let maximum = scroll.max_offset().y;

	if maximum <= gpui::px(0.) || viewport <= gpui::px(0.) {
		return None;
	}

	let height = (bounds.size.height * (viewport / (viewport + maximum)))
		.max(gpui::px(28.))
		.min(bounds.size.height);
	let travel = bounds.size.height - height;
	let y = travel * (-scroll.offset().y / maximum);
	let thumb = Bounds::new(
		gpui::point(bounds.center().x - thumb_width / 2., bounds.top() + y),
		gpui::size(thumb_width, height),
	);

	Some((scroll, thumb, travel, window.insert_hitbox(bounds, HitboxBehavior::Normal)))
}

fn should_smooth(delta: ScrollDelta, enabled: bool) -> bool {
	enabled && matches!(delta, ScrollDelta::Lines(_))
}

fn set_scroll(scroll: &ScrollHandle, position: Pixels, travel: Pixels) {
	if travel > gpui::px(0.) {
		let fraction = (position / travel).clamp(0., 1.);

		scroll.set_offset(gpui::point(gpui::px(0.), -scroll.max_offset().y * fraction));
	}
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

		for hz in [60, 120, 144, 165, 240] {
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
