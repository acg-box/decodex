//! Interruptible motion shared by native controls and workspace panels.
#[path = "ui_overflow_label.rs"] mod overflow_label;
pub(crate) use overflow_label::OverflowLabel;
#[path = "ui_text_reveal.rs"] mod text_reveal;
pub(crate) use text_reveal::TextReveal;

use std::time::{Duration, Instant};

pub(crate) use crate::ui_frame::{
	animations_active, application_active, observe_activation, request_frame, window_options,
};

use gpui::{
	AnyElement, App, Div, Element, ElementId, IntoElement, MouseButton, RenderOnce, Stateful,
	Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
#[cfg(all(target_os = "macos", not(test)))]
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject},
};

pub(crate) trait SmoothControl {
	fn smooth(self) -> Control;
}

/// Collapse the whole tab footprint, including its gutter, without squeezing its label.
#[derive(IntoElement)]
pub(crate) struct TabReveal {
	pub id: ElementId,
	pub visible: bool,
	pub child: AnyElement,
	pub closed: Box<dyn FnOnce(&mut App)>,
}
impl RenderOnce for TabReveal {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| Tween::new(0.));
		let now = Instant::now();
		let (height, moving) = state.update(cx, |s, _| {
			s.target(
				if self.visible {
					crate::ui_theme::CONVERSATION_TAB_SIZE + crate::ui_theme::CONVERSATION_TAB_GAP
				} else {
					0.
				},
				now,
			);
			(s.sample(now), s.moving(now))
		});
		if moving {
			request_frame(window, cx);
		}
		if !self.visible && !moving {
			cx.defer(self.closed);
		}
		gpui::div().w_full().h(gpui::px(height)).flex_none().overflow_hidden().child(self.child)
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

impl RenderOnce for Control {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		if !self.enabled {
			return self.div.into_any_element();
		}

		let id = Element::id(&self.div).expect("animated controls require an id");
		let state = window.use_keyed_state(id, cx, |_, _| Feedback {
			hovered: false,
			pressed: false,
			opacity: Tween::new(0.96),
		});
		let now = Instant::now();
		let feedback = state.read(cx);
		let moving = feedback.opacity.moving(now);
		let opacity = feedback.opacity.sample(now);

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

					s.opacity.from = 0.90;
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
pub(crate) struct Disclosure {
	id: ElementId,
	visible: bool,
	child: Box<dyn FnOnce(&mut App) -> AnyElement>,
}
impl RenderOnce for Disclosure {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| DisclosureState {
			visible: false,
			height: 0.0,
			tween: Tween::new(0.0),
		});
		let now = Instant::now();
		let (height, moving) = state.update(cx, |s, _| {
			if self.visible && s.visible && s.tween.to > 0. && !s.tween.moving(now) {
				// Once open, let inner disclosures and new content own their size.
				// Animating both parent and child would make the parent lag behind.
				s.tween = Tween::new(s.height);
			} else {
				s.tween.target(if self.visible { s.height } else { 0.0 }, now);
			}

			s.visible = self.visible;

			(s.tween.sample(now), s.tween.moving(now))
		});

		if moving {
			request_frame(window, cx);
		}

		gpui::div()
			.w_full()
			.h(gpui::px(height))
			.when(!self.visible && height <= 0.1, |slot| slot.hidden())
			.when(self.visible && !moving && height > 0.1, |slot| slot.h_auto())
			.flex_none()
			.overflow_hidden()
			.when(self.visible || height > 0.1, |slot| {
				slot.child(
					gpui::div()
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
						.child((self.child)(cx)),
				)
			})
	}
}

/// A short arrival transition, keyed by route rather than background refreshes.
#[derive(IntoElement)]
pub(crate) struct Arrival {
	route: String,
	child: AnyElement,
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

		gpui::div()
			.size_full()
			.min_h_0()
			.relative()
			.flex()
			.flex_col()
			.top(gpui::px(4.0 * (1.0 - progress)))
			.opacity(0.85 + 0.15 * progress)
			.child(self.child)
	}
}

/// Float menus into place without clipping their shadow or fading individual primitives.
#[derive(IntoElement)]
pub(crate) struct Popover {
	id: ElementId,
	unframed: bool,
	visible: bool,
	child: AnyElement,
}
impl Popover {
	pub(crate) fn unframed(mut self, unframed: bool) -> Self {
		self.unframed = unframed;
		self
	}
}
impl RenderOnce for Popover {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| Tween::new(0.));
		let now = Instant::now();
		let (progress, moving) = state.update(cx, |s, _| {
			if !self.visible {
				*s = Tween::new(0.);
			} else {
				s.target(1., now);
			}
			(s.sample(now), s.moving(now))
		});
		if moving {
			request_frame(window, cx);
		}

		// A disclosure's height clip cuts the blur into a rectangular dark patch.
		// Menus overlay the page, so animate their position without a layout clip.
		gpui::div().w_full().when(self.visible, |slot| {
			slot.p_2().child(
				gpui::div()
					.w_full()
					.relative()
					.top(gpui::px(-4. * (1. - progress)))
					.when(!self.unframed, menu_surface)
					.child(self.child),
			)
		})
	}
}

/// Hover text must remain readable over arbitrary conversation content.
pub(crate) fn tooltip_surface(surface: gpui::Div) -> gpui::Div {
	surface
		.rounded(gpui::px(8.))
		.border_1()
		.border_color(crate::ui_theme::rgba(0xffffff24))
		.bg(crate::ui_theme::rgb(0x24242a))
		.text_color(crate::ui_theme::rgb(crate::ui_theme::TEXT))
		.text_size(gpui::px(11.))
		.line_height(gpui::px(16.))
		.max_w(gpui::px(360.))
}

/// Shared matte menu surface with a quiet outline and compact elevation.
pub(crate) fn menu_surface<T: gpui::Styled>(surface: T) -> T {
	surface
		.rounded(gpui::px(10.))
		.border_1()
		.border_color(crate::ui_theme::rgba(0xffffff14))
		.bg(crate::ui_theme::rgb(crate::ui_theme::SURFACE_OVERLAY_MATERIAL >> 8))
		.shadow(vec![gpui::BoxShadow {
			inset: false,
			color: crate::ui_theme::rgba(0x00000028).into(),
			offset: gpui::point(gpui::px(0.), gpui::px(3.)),
			blur_radius: gpui::px(8.),
			spread_radius: gpui::px(-2.),
		}])
}

#[derive(Clone)]
pub(crate) struct Tween {
	from: f32,
	to: f32,
	started: Instant,
	duration: Duration,
}
impl Tween {
	pub(crate) fn duration(mut self, duration: Duration) -> Self {
		self.duration = duration;
		self
	}

	pub(crate) fn new(value: f32) -> Self {
		Self {
			from: value,
			to: value,
			started: Instant::now(),
			duration: Duration::from_millis(200),
		}
	}

	pub(crate) fn sample(&self, now: Instant) -> f32 {
		if self.from == self.to || now.duration_since(self.started) >= self.duration {
			return self.to;
		}
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

	pub(crate) fn target(&mut self, value: f32, now: Instant) {
		if self.to == value
			&& (self.from == self.to || now.duration_since(self.started) >= self.duration)
		{
			// A completed transition is static. Do not query AppKit again for
			// every unrelated frame until the target actually changes.
			self.from = value;
			return;
		}
		if (self.from != self.to || self.to != value) && reduced() {
			self.from = value;
			self.to = value;
			self.started = now;
		} else if self.to != value {
			self.from = self.sample(now);
			self.to = value;
			self.started = now;
		}
	}

	pub(crate) fn moving(&self, now: Instant) -> bool {
		self.from != self.to && now.duration_since(self.started) < self.duration && !reduced()
	}
}

struct Feedback {
	hovered: bool,
	pressed: bool,
	opacity: Tween,
}
impl Feedback {
	fn update(&mut self) {
		self.opacity.target(
			if self.pressed {
				0.90
			} else if self.hovered {
				1.0
			} else {
				0.96
			},
			Instant::now(),
		);
	}
}

struct DisclosureState {
	visible: bool,
	height: f32,
	tween: Tween,
}

impl SmoothControl for Stateful<Div> {
	fn smooth(self) -> Control {
		Control { div: self, enabled: true }
	}
}

/// Respect system accessibility preferences and the application rendering policy.
pub(crate) fn reduced() -> bool {
	if !animations_active() {
		return true;
	}
	#[cfg(all(target_os = "macos", not(test)))]
	{
		// AppKit owns this process-wide preference; called from UI rendering.
		unsafe {
			let workspace: Retained<AnyObject> =
				objc2::msg_send![AnyClass::get(c"NSWorkspace").expect("AppKit"), sharedWorkspace];
			let reduce_motion: bool =
				objc2::msg_send![&*workspace, accessibilityDisplayShouldReduceMotion];
			let voice_over: bool = objc2::msg_send![&*workspace, isVoiceOverEnabled];

			reduce_motion || voice_over
		}
	}

	#[cfg(any(not(target_os = "macos"), test))]
	false
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

pub(crate) fn disclosure(
	id: impl Into<ElementId>,
	visible: bool,
	child: impl IntoElement,
) -> Disclosure {
	let child = child.into_any_element();

	disclosure_lazy(id, visible, move |_| child)
}

/// Do not construct Markdown and tool details for fully collapsed processes.
pub(crate) fn disclosure_lazy(
	id: impl Into<ElementId>,
	visible: bool,
	child: impl FnOnce(&mut App) -> AnyElement + 'static,
) -> Disclosure {
	Disclosure { id: id.into(), visible, child: Box::new(child) }
}

pub(crate) fn arrival(route: String, child: impl IntoElement) -> Arrival {
	Arrival { route, child: child.into_any_element() }
}

pub(crate) fn popover(id: impl Into<ElementId>, visible: bool, child: impl IntoElement) -> Popover {
	Popover { id: id.into(), visible, child: child.into_any_element(), unframed: false }
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use crate::ui_motion::Tween;

	#[test]
	fn reduced_motion_finishes_an_in_progress_transition_immediately() {
		let tween = Tween::new(0.0);
		let start = Instant::now();
		let mut tween = tween;

		tween.target(192.0, start);

		let middle = start + Duration::from_millis(80);

		assert!(tween.sample_with_motion(middle, false) < 192.0);
		assert_eq!(tween.sample_with_motion(middle, true), 192.0);

		tween.target(0.0, middle);

		assert_eq!(tween.sample_with_motion(middle, true), 0.0);
	}

	#[test]
	fn reversing_a_transition_preserves_current_position_and_settles() {
		let tween = Tween::new(0.0);
		let start = Instant::now();
		let mut tween = tween;

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

	#[test]
	fn completed_targets_settle_and_retarget_without_a_position_jump() {
		let mut tween = Tween::new(0.);
		let start = Instant::now();
		tween.target(1., start);
		let end = start + Duration::from_millis(220);
		tween.target(1., end);
		assert_eq!(tween.from, tween.to);
		assert!(!tween.moving(end));
		tween.target(0., end);
		assert_eq!(tween.sample(end), 1.);
		let middle = end + Duration::from_millis(60);
		let position = tween.sample(middle);
		tween.target(1., middle);
		assert_eq!(tween.sample(middle), position);
		assert!(tween.moving(middle));
	}
}

/// The title's first grapheme carries state without adding a badge or changing layout.
#[derive(IntoElement)]
pub(crate) struct AgentRailStatus {
	pub id: String,
	pub state: String,
	pub label: String,
}
impl RenderOnce for AgentRailStatus {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let (color, busy) = match self.state.as_str() {
			"Running" | "Starting" | "running" | "working" | "active" =>
				(crate::ui_theme::INFO, true),
			"Needs you" | "Needs attention" | "Blocked" => (crate::ui_theme::WARNING, false),
			"failed" | "systemError" | "Failed" => (crate::ui_theme::ERROR, false),
			"Completed" | "completed" | "Resolved" => (crate::ui_theme::SUCCESS, false),
			_ => (crate::ui_theme::TEXT, false),
		};
		let color = crate::ui_theme::ColorScheme::current().hex(color);
		let mut channels = [0.; 3];
		for (index, channel) in channels.iter_mut().enumerate() {
			*channel = value(
				gpui::SharedString::from(format!("rail-color-{}-{index}", self.id)),
				((color >> (16 - index * 8)) & 255) as f32 / 255.,
				window,
				cx,
			);
		}
		// No completion percentage exists for a model turn. A quiet change in
		// the letter's ink indicates activity without implying measured progress.
		let clock = window.use_keyed_state(
			gpui::SharedString::from(format!("rail-activity-{}", self.id)),
			cx,
			|_, _| Instant::now(),
		);
		let intensity = if busy && !reduced() {
			request_frame(window, cx);
			0.82 + 0.18
				* (clock.read(cx).elapsed().as_secs_f32() * std::f32::consts::TAU / 2.4).cos()
		} else {
			1.
		};
		let ink = gpui::Rgba {
			r: channels[0] * intensity,
			g: channels[1] * intensity,
			b: channels[2] * intensity,
			a: 1.,
		};

		use unicode_segmentation::UnicodeSegmentation as _;
		let label = self.label.replace(['\n', '\r'], " ");
		let initial = label.graphemes(true).next().unwrap_or("·").to_owned();
		let rest = label.get(initial.len()..).unwrap_or("").to_owned();
		let mut font = window.text_style().font();
		font.weight = gpui::FontWeight::SEMIBOLD;
		let run = gpui::TextRun {
			len: initial.len(),
			font,
			color: ink.into(),
			background_color: None,
			underline: None,
			strikethrough: None,
		};
		let initial_width = f32::from(
			window
				.text_system()
				.shape_line(initial.clone().into(), gpui::px(12.5), &[run], None)
				.width,
		);
		let inset = ((crate::ui_theme::CONVERSATION_TAB_SIZE - initial_width) * 0.5).max(0.);
		let title_id = gpui::SharedString::from(format!("left-title-{}", self.id));
		gpui::div()
			.debug_selector(|| "conversation-name".into())
			.min_w(gpui::px(crate::ui_theme::CONVERSATION_TAB_SIZE))
			.h_full()
			.flex_1()
			.relative()
			.flex()
			.items_center()
			.pl(gpui::px(inset))
			.pr(gpui::px(inset))
			.text_size(gpui::px(12.5))
			.child(
				gpui::div()
					.id(gpui::SharedString::from(format!("conversation-mark-{}", self.id)))
					.debug_selector(move || format!("conversation-mark-{}", self.id))
					.role(gpui::Role::Status)
					.aria_label(self.state)
					.w(gpui::px(initial_width))
					.flex_none()
					.font_weight(gpui::FontWeight::SEMIBOLD)
					.text_color(ink)
					.child(initial),
			)
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.overflow_hidden()
					.child(OverflowLabel { id: title_id.into(), text: rest.into() }),
			)
	}
}
