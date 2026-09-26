//! Interruptible motion shared by native controls and workspace panels.
use std::time::{Duration, Instant};

use gpui::{
	App, Div, Element, ElementId, IntoElement, MouseButton, RenderOnce, Stateful, Window, div,
	prelude::*, px,
};

/// Follow host reduced-motion and VoiceOver preferences without changing configuration.
pub(crate) fn reduced() -> bool {
	#[cfg(all(target_os = "macos", not(test)))]
	{
		use objc2::{
			msg_send,
			rc::Retained,
			runtime::{AnyClass, AnyObject},
		};
		// AppKit owns this process-wide preference; called from UI rendering.
		unsafe {
			let workspace: Retained<AnyObject> =
				msg_send![AnyClass::get(c"NSWorkspace").expect("AppKit"), sharedWorkspace];
			let reduce_motion: bool =
				msg_send![&*workspace, accessibilityDisplayShouldReduceMotion];
			let voice_over: bool = msg_send![&*workspace, isVoiceOverEnabled];
			reduce_motion || voice_over
		}
	}
	#[cfg(any(not(target_os = "macos"), test))]
	false
}

/// Keep animation cadence shared by a workspace and its attached controls.
pub(crate) fn request_frame(window: &Window, cx: &mut App) {
	#[cfg(all(target_os = "macos", not(test)))]
	if crate::ui_theme::native_glass_panel::request_workspace_frame(window, cx) {
		return;
	}
	let _ = cx;
	window.request_animation_frame();
}

#[derive(Clone)]
struct Tween {
	from: f32,
	to: f32,
	started: Instant,
	duration: Duration,
}

impl Tween {
	fn new(value: f32) -> Self {
		Self {
			from: value,
			to: value,
			started: Instant::now(),
			duration: Duration::from_millis(200),
		}
	}

	fn sample(&self, now: Instant) -> f32 {
		self.sample_with_motion(now, reduced())
	}

	fn sample_with_motion(&self, now: Instant, reduced: bool) -> f32 {
		if reduced {
			return self.to;
		}
		let t =
			(now.duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32()).min(1.0);
		let eased = 1.0 - (1.0 - t).powi(3);
		self.from + (self.to - self.from) * eased
	}

	fn target(&mut self, value: f32, now: Instant) {
		if self.to != value {
			self.from = self.sample(now);
			self.to = value;
			self.started = now;
		}
	}

	fn moving(&self, now: Instant) -> bool {
		!reduced() && self.from != self.to && now.duration_since(self.started) < self.duration
	}
}

/// Animate a native overlay as one composited surface, including its shadow.
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) fn native_presence(
	id: &'static str,
	visible: bool,
	window: &mut Window,
	cx: &mut App,
) -> f32 {
	let state = window.use_keyed_state(id, cx, |_, _| Tween::new(0.));
	let now = Instant::now();
	let (opacity, moving) = state.update(cx, |s, _| {
		s.duration = Duration::from_millis(180);
		s.target(if visible { 1. } else { 0. }, now);
		(s.sample(now), s.moving(now))
	});
	if moving {
		request_frame(window, cx);
	}
	opacity
}

pub(crate) fn value(
	id: impl Into<ElementId>,
	target: f32,
	window: &mut Window,
	cx: &mut App,
) -> f32 {
	direct_value(id, target, false, window, cx)
}

/// Track direct input without lag, then ease to the released target.
pub(crate) fn direct_value(
	id: impl Into<ElementId>,
	target: f32,
	direct: bool,
	window: &mut Window,
	cx: &mut App,
) -> f32 {
	let state = window.use_keyed_state(id.into(), cx, |_, _| Tween::new(target));
	let now = Instant::now();
	let (value, moving) = state.update(cx, |s, _| {
		if direct {
			*s = Tween::new(target);
		}
		s.target(target, now);
		(s.sample(now), s.moving(now))
	});
	if moving {
		request_frame(window, cx);
	}
	value
}

#[derive(IntoElement)]
pub(crate) struct Reveal {
	id: ElementId,
	extent: f32,
	horizontal: bool,
	child: gpui::AnyElement,
}

pub(crate) fn reveal(
	id: impl Into<ElementId>,
	extent: f32,
	horizontal: bool,
	child: impl IntoElement,
) -> Reveal {
	Reveal { id: id.into(), extent, horizontal, child: child.into_any_element() }
}

impl RenderOnce for Reveal {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| Tween::new(self.extent));
		let now = Instant::now();
		let (extent, moving) = state.update(cx, |s, _| {
			s.target(self.extent, now);
			(s.sample(now), s.moving(now))
		});
		if moving {
			request_frame(window, cx);
		}
		div()
			.flex_none()
			.overflow_hidden()
			.when(self.horizontal, |slot| slot.w(px(extent)).h_full())
			.when(!self.horizontal, |slot| slot.h(px(extent)).w_full())
			.when(extent > 0.1, |slot| slot.child(self.child))
	}
}

#[derive(IntoElement)]
pub(crate) struct Control {
	div: Stateful<Div>,
	enabled: bool,
}

impl Control {
	pub(crate) fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = enabled;
		self
	}
}

pub(crate) trait SmoothControl {
	fn smooth(self) -> Control;
}

impl SmoothControl for Stateful<Div> {
	fn smooth(self) -> Control {
		Control { div: self, enabled: true }
	}
}

struct Feedback {
	hovered: bool,
	pressed: bool,
	opacity: Tween,
	offset: Tween,
}

impl Feedback {
	fn update(&mut self) {
		self.offset.target(if self.pressed { 1.5 } else { 0.0 }, Instant::now());
		self.opacity.target(
			if self.pressed {
				0.62
			} else if self.hovered {
				1.0
			} else {
				0.88
			},
			Instant::now(),
		);
	}
}

impl RenderOnce for Control {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		if !self.enabled {
			return self.div.into_any_element();
		}
		let id = Element::id(&self.div).expect("animated controls require an id");
		let state = window.use_keyed_state(id, cx, |_, _| Feedback {
			hovered: false,
			pressed: false,
			opacity: Tween::new(0.88),
			offset: Tween::new(0.0),
		});
		let now = Instant::now();
		let feedback = state.read(cx);
		let moving = feedback.opacity.moving(now) || feedback.offset.moving(now);
		let opacity = feedback.opacity.sample(now);
		let offset = feedback.offset.sample(now);
		if moving {
			request_frame(window, cx);
		}
		let hover = state.clone();
		let down = state.clone();
		let up = state.clone();
		let outside = state.clone();
		let key_down = state.clone();
		let click = state.clone();
		self.div
			.relative()
			.top(px(offset))
			.opacity(opacity)
			.on_hover(move |hovered, _, cx| {
				hover.update(cx, |s, cx| {
					s.hovered = *hovered;
					if !hovered {
						s.pressed = false;
					}
					s.update();
					cx.notify();
				})
			})
			.on_mouse_down(MouseButton::Left, move |_, _, cx| {
				down.update(cx, |s, cx| {
					s.pressed = true;
					s.update();
					cx.notify();
				})
			})
			.on_mouse_up(MouseButton::Left, move |_, _, cx| {
				up.update(cx, |s, cx| {
					s.pressed = false;
					s.update();
					cx.notify();
				})
			})
			.on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
				outside.update(cx, |s, cx| {
					s.pressed = false;
					s.update();
					cx.notify();
				})
			})
			.on_key_down(move |event, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					key_down.update(cx, |s, cx| {
						s.pressed = true;
						s.update();
						cx.notify();
					});
				}
			})
			.on_click(move |_, _, cx| {
				click.update(cx, |s, cx| {
					s.pressed = false;
					s.update();
					s.offset.from = 1.5;
					s.offset.to = 0.0;
					s.offset.started = Instant::now();
					s.opacity.from = 0.62;
					s.opacity.started = Instant::now();
					cx.notify();
				})
			})
			.on_key_up(move |_, _, cx| {
				state.update(cx, |s, cx| {
					s.pressed = false;
					s.update();
					cx.notify();
				})
			})
			.into_any_element()
	}
}

#[derive(IntoElement)]
pub(crate) struct SwitchKnob {
	id: &'static str,
	enabled: bool,
	child: gpui::AnyElement,
}

pub(crate) fn switch_knob(id: &'static str, enabled: bool, child: impl IntoElement) -> SwitchKnob {
	SwitchKnob { id, enabled, child: child.into_any_element() }
}

impl RenderOnce for SwitchKnob {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let target = if self.enabled { 16.0 } else { 0.0 };
		let state = window.use_keyed_state(self.id, cx, |_, _| Tween::new(target));
		let now = Instant::now();
		let offset = state.update(cx, |s, _| {
			s.target(target, now);
			s.sample(now)
		});
		if state.read(cx).moving(now) {
			request_frame(window, cx);
		}
		div().ml(px(offset)).size(px(14.0)).child(self.child)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn reduced_motion_finishes_an_in_progress_transition_immediately() {
		let mut tween = Tween::new(0.0);
		let start = Instant::now();
		tween.target(192.0, start);
		let middle = start + Duration::from_millis(80);
		assert!(tween.sample_with_motion(middle, false) < 192.0);
		assert_eq!(tween.sample_with_motion(middle, true), 192.0);
		tween.target(0.0, middle);
		assert_eq!(tween.sample_with_motion(middle, true), 0.0);
	}

	#[test]
	fn reversing_a_transition_preserves_current_position_and_settles() {
		let mut tween = Tween::new(0.0);
		let start = Instant::now();
		tween.target(192.0, start);
		let middle = start + Duration::from_millis(80);
		let position = tween.sample(middle);
		assert!(position > 0.0 && position < 192.0);
		tween.target(0.0, middle);
		assert_eq!(position, tween.sample(middle));
		let end = middle + Duration::from_millis(220);
		assert_eq!(tween.sample(end), 0.0);
		assert!(!tween.moving(end));
	}
}

#[derive(IntoElement)]
pub(crate) struct Disclosure {
	id: &'static str,
	visible: bool,
	child: gpui::AnyElement,
}

pub(crate) fn disclosure(id: &'static str, visible: bool, child: impl IntoElement) -> Disclosure {
	Disclosure { id, visible, child: child.into_any_element() }
}

struct DisclosureState {
	height: f32,
	tween: Tween,
}

impl RenderOnce for Disclosure {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| DisclosureState {
			height: 0.0,
			tween: Tween::new(0.0),
		});
		let now = Instant::now();
		let (height, moving) = state.update(cx, |s, _| {
			s.tween.target(if self.visible { s.height } else { 0.0 }, now);
			(s.tween.sample(now), s.tween.moving(now))
		});
		if moving {
			request_frame(window, cx);
		}
		div().w_full().h(px(height)).flex_none().overflow_hidden().when(
			self.visible || height > 0.1,
			|slot| {
				slot.child(
					div()
						.w_full()
						.flex_none()
						.on_children_prepainted(move |bounds, _, cx| {
							if let Some(bounds) = bounds.first() {
								let measured = f32::from(bounds.size.height);
								state.update(cx, |s, cx| {
									if (s.height - measured).abs() > 0.5 {
										s.height = measured;
										cx.notify();
									}
								});
							}
						})
						.child(self.child),
				)
			},
		)
	}
}

/// A short arrival transition, keyed by route rather than background refreshes.
#[derive(IntoElement)]
pub(crate) struct Arrival {
	route: String,
	child: gpui::AnyElement,
}

pub(crate) fn arrival(route: String, child: impl IntoElement) -> Arrival {
	Arrival { route, child: child.into_any_element() }
}

impl RenderOnce for Arrival {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window
			.use_keyed_state("page-arrival", cx, |_, _| (self.route.clone(), Tween::new(1.0)));
		let now = Instant::now();
		let (progress, moving) = state.update(cx, |s, _| {
			if s.0 != self.route {
				s.0 = self.route;
				s.1 = Tween::new(0.0);
				s.1.target(1.0, now);
			}
			(s.1.sample(now), s.1.moving(now))
		});
		if moving {
			request_frame(window, cx);
		}
		div()
			.size_full()
			.min_h_0()
			.relative()
			.flex()
			.flex_col()
			.top(px(4.0 * (1.0 - progress)))
			.opacity(0.85 + 0.15 * progress)
			.child(self.child)
	}
}

/// A content-sized popover that fades without stretching or clipping its contents.
#[derive(IntoElement)]
pub(crate) struct Popover {
	unframed: bool,
	id: &'static str,
	kind: &'static str,
	visible: bool,
	child: gpui::AnyElement,
}
pub(crate) fn popover(
	id: &'static str,
	kind: &'static str,
	visible: bool,
	child: impl IntoElement,
) -> Popover {
	Popover { id, kind, visible, child: child.into_any_element(), unframed: false }
}
impl Popover {
	pub(crate) fn unframed(mut self, unframed: bool) -> Self {
		self.unframed = unframed;
		self
	}
}
impl RenderOnce for Popover {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| (self.kind, Tween::new(0.)));
		let now = Instant::now();
		let (opacity, moving) = state.update(cx, |s, _| {
			if s.0 != self.kind {
				s.0 = self.kind;
				s.1 = Tween::new(0.);
			}
			s.1.duration = Duration::from_millis(180);
			s.1.target(if self.visible { 1. } else { 0. }, now);
			(s.1.sample(now), s.1.moving(now))
		});
		if moving {
			request_frame(window, cx);
		}
		if opacity <= 0.001 {
			return div().w_full().into_any_element();
		}
		div()
			.w_full()
			.relative()
			.top(px((1. - opacity) * 4.))
			.opacity(opacity)
			.when(!self.unframed, |surface| {
				surface.rounded(px(14.)).bg(gpui::rgb(0x29292d)).shadow(vec![gpui::BoxShadow {
					inset: false,
					color: gpui::rgba(0x00000024).into(),
					offset: gpui::point(px(0.), px(4.)),
					blur_radius: px(12.),
					spread_radius: px(-3.),
				}])
			})
			.child(self.child)
			.into_any_element()
	}
}
