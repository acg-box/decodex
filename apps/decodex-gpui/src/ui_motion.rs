//! Interruptible motion shared by native controls and workspace panels.

#[cfg(all(target_os = "macos", not(test)))]
#[path = "ui_native_popover.rs"]
mod native_popover;
#[path = "ui_overflow_label.rs"] mod overflow_label;
#[path = "ui_segmented.rs"] mod segmented;
#[path = "ui_text_reveal.rs"] mod text_reveal;

pub(crate) use crate::ui_frame::{
	animations_active, application_active, observe_activation, reduced, request_frame,
	window_options,
};
pub(crate) use overflow_label::OverflowLabel;
pub(crate) use segmented::{Choice, Segmented};
pub(crate) use text_reveal::TextReveal;

use std::{
	f32::consts::TAU,
	time::{Duration, Instant},
};

use gpui::{
	AnyElement, App, Div, Element, ElementId, FontWeight, IntoElement, MouseButton, RenderOnce,
	Rgba, Role, SharedString, Stateful, TextRun, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled,
	},
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::ui_theme::{
	self, CONVERSATION_TAB_GAP, CONVERSATION_TAB_SIZE, ColorScheme, ERROR, INFO, SUCCESS, TEXT,
	WARNING,
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
				if self.visible { CONVERSATION_TAB_SIZE + CONVERSATION_TAB_GAP } else { 0. },
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

/// Resolve final dimensions before the complete surface arrives.
#[derive(IntoElement)]
pub(crate) struct Popover {
	id: ElementId,
	visible: bool,
	child: Option<AnyElement>,
	content: Option<std::rc::Rc<PopoverContent>>,
	dismiss: std::rc::Rc<dyn Fn(&mut App)>,
	trigger: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
	above: bool,
	preview: bool,
	focus: Option<gpui::FocusHandle>,
}

type PopoverContent = dyn Fn(&mut Window, &mut App) -> AnyElement;

/// A persistent floating control reuses the same native material as transient menus.
#[derive(IntoElement)]
pub(crate) struct MaterialSurface {
	id: ElementId,
	radius: f64,
	content: std::rc::Rc<PopoverContent>,
	dismiss: std::rc::Rc<dyn Fn(&mut App)>,
}
impl RenderOnce for MaterialSurface {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		#[cfg(all(target_os = "macos", not(test)))]
		if native_popover::available() {
			return native_popover::render_surface(
				self.id,
				self.radius,
				self.content,
				self.dismiss,
				window,
				cx,
			);
		}
		let _ = (&self.id, &self.dismiss);
		gpui::div()
			.size_full()
			.rounded(gpui::px(self.radius as f32))
			.bg(ui_theme::popover_surface())
			.child((self.content)(window, cx))
			.into_any_element()
	}
}

pub(crate) fn material_surface(
	id: impl Into<ElementId>,
	radius: f64,
	content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
	dismiss: impl Fn(&mut App) + 'static,
) -> MaterialSurface {
	MaterialSurface {
		id: id.into(),
		radius,
		content: std::rc::Rc::new(content),
		dismiss: std::rc::Rc::new(dismiss),
	}
}

pub(crate) fn hide_material_surface(id: impl Into<ElementId>, window: &Window, cx: &mut App) {
	let parent = window.window_handle();
	let id = id.into();
	#[cfg(all(target_os = "macos", not(test)))]
	cx.defer(move |cx| native_popover::hide_surface(parent, id, cx));
	#[cfg(not(all(target_os = "macos", not(test))))]
	let _ = (parent, id, cx);
}

#[cfg(all(target_os = "macos", not(test)))]
pub(crate) fn claim_popover(
	parent: gpui::AnyWindowHandle,
	popup: gpui::AnyWindowHandle,
	dismiss: impl Fn(&mut App) + 'static,
	cx: &mut App,
) {
	native_popover::claim(parent, popup, std::rc::Rc::new(dismiss), cx);
}

#[cfg(all(target_os = "macos", not(test)))]
pub(crate) fn dismiss_shared_popover(parent: gpui::AnyWindowHandle, cx: &mut App) {
	native_popover::dismiss_host(parent, cx);
}

/// A hover owner's release closes only its own native preview.
pub(crate) fn dismiss_popover(parent: gpui::AnyWindowHandle, id: ElementId, cx: &mut App) {
	#[cfg(all(target_os = "macos", not(test)))]
	native_popover::dismiss(parent, id, cx);
	#[cfg(not(all(target_os = "macos", not(test))))]
	let _ = (parent, id, cx);
}

impl Popover {
	pub(crate) fn on_dismiss(mut self, dismiss: impl Fn(&mut App) + 'static) -> Self {
		self.dismiss = std::rc::Rc::new(dismiss);
		self
	}

	pub(crate) fn trigger(
		mut self,
		bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
	) -> Self {
		self.trigger = bounds;
		self
	}

	pub(crate) fn above(mut self) -> Self {
		self.above = true;
		self
	}

	pub(crate) fn preview(mut self) -> Self {
		self.preview = true;
		self
	}

	pub(crate) fn focus(mut self, focus: Option<gpui::FocusHandle>) -> Self {
		self.focus = focus;
		self
	}
}

impl RenderOnce for Popover {
	fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		#[cfg(all(target_os = "macos", not(test)))]
		if self.content.is_some() && native_popover::available() {
			return native_popover::render(self, window, cx);
		}
		let child = if let Some(content) = self.content.take() {
			content(window, cx)
		} else {
			self.child.take().expect("popover content")
		};
		let arrival = window.use_keyed_state(self.id.clone(), cx, |_, _| {
			Tween::new(0.).duration(ui_theme::POPOVER_ARRIVAL)
		});
		let now = Instant::now();
		let (opacity, moving) = arrival.update(cx, |arrival, _| {
			if !self.visible {
				*arrival = Tween::new(0.).duration(ui_theme::POPOVER_ARRIVAL);
				return (0., false);
			}
			arrival.target(1., now);
			(arrival.sample(now), arrival.moving(now))
		});
		if moving {
			request_frame(window, cx);
		}
		// The complete surface arrives together; its size and text scale stay fixed.
		gpui::div()
			.id(self.id)
			.w_full()
			.when(self.visible, |slot| {
				slot.when(!self.preview, |slot| {
					slot.px(gpui::px(ui_theme::POPOVER_PADDING)).py(gpui::px(ui_theme::POPOVER_GAP))
				})
				.child(
					gpui::div()
						.w_full()
						.relative()
						.top(gpui::px(ui_theme::POPOVER_OFFSET * (1. - opacity)))
						.opacity(opacity)
						.debug_selector(|| "popover-surface".into())
						.map(menu_surface)
						.child(child),
				)
			})
			.into_any_element()
	}
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
			"Running" | "Starting" | "running" | "working" | "active" => (INFO, true),
			"Needs you" | "Needs attention" | "Blocked" => (WARNING, false),
			"failed" | "systemError" | "Failed" => (ERROR, false),
			"Completed" | "completed" | "Resolved" => (SUCCESS, false),
			_ => (TEXT, false),
		};
		let color = ColorScheme::current().hex(color);
		let mut channels = [0.; 3];

		for (index, channel) in channels.iter_mut().enumerate() {
			*channel = value(
				SharedString::from(format!("rail-color-{}-{index}", self.id)),
				((color >> (16 - index * 8)) & 255) as f32 / 255.,
				window,
				cx,
			);
		}

		// No completion percentage exists for a model turn. A quiet change in
		// the letter's ink indicates activity without implying measured progress.
		let clock = window.use_keyed_state(
			SharedString::from(format!("rail-activity-{}", self.id)),
			cx,
			|_, _| Instant::now(),
		);
		let intensity = if busy && !reduced() {
			request_frame(window, cx);

			0.82 + 0.18 * (clock.read(cx).elapsed().as_secs_f32() * TAU / 2.4).cos()
		} else {
			1.
		};
		let ink = Rgba {
			r: channels[0] * intensity,
			g: channels[1] * intensity,
			b: channels[2] * intensity,
			a: 1.,
		};
		let label = self.label.replace(['\n', '\r'], " ");
		let initial = label.graphemes(true).next().unwrap_or("·").to_owned();
		let rest = label.get(initial.len()..).unwrap_or("").to_owned();
		let mut font = window.text_style().font();

		font.weight = FontWeight::SEMIBOLD;

		let run = TextRun {
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
		let inset = ((CONVERSATION_TAB_SIZE - initial_width) * 0.5).max(0.);
		let title_id = SharedString::from(format!("left-title-{}", self.id));

		gpui::div()
			.debug_selector(|| "conversation-name".into())
			.min_w(gpui::px(CONVERSATION_TAB_SIZE))
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
					.id(SharedString::from(format!("conversation-mark-{}", self.id)))
					.debug_selector(move || format!("conversation-mark-{}", self.id))
					.role(Role::Status)
					.aria_label(self.state)
					.w(gpui::px(initial_width))
					.flex_none()
					.font_weight(FontWeight::SEMIBOLD)
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

/// Hover text must remain readable over arbitrary conversation content.
pub(crate) fn tooltip_surface(surface: Div) -> Div {
	surface
		.rounded(gpui::px(8.))
		.border(gpui::px(0.5))
		.border_color(ui_theme::rgb(TEXT).opacity(0.1))
		.bg(ui_theme::popover_surface())
		.px(gpui::px(8.))
		.py(gpui::px(6.))
		.map(popover_elevation)
		.text_color(ui_theme::rgb(TEXT))
		.text_size(gpui::px(11.))
		.line_height(gpui::px(16.))
		.max_w(gpui::px(360.))
}

/// Shared matte menu surface with a quiet outline and compact elevation.
pub(crate) fn menu_surface<T>(surface: T) -> T
where
	T: Styled,
{
	popover_elevation(ui_theme::menu_outline(surface).bg(ui_theme::popover_surface()))
}

fn popover_elevation<T: Styled>(surface: T) -> T {
	surface.shadow(vec![gpui::BoxShadow {
		inset: false,
		color: ui_theme::rgba(0x00000020).into(),
		offset: gpui::point(gpui::px(0.), gpui::px(2.)),
		blur_radius: gpui::px(10.),
		spread_radius: gpui::px(-1.),
	}])
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
	Popover {
		id: id.into(),
		visible,
		child: Some(child.into_any_element()),
		content: None,
		dismiss: std::rc::Rc::new(|_| {}),
		trigger: Default::default(),
		above: false,
		preview: false,
		focus: None,
	}
}

/// Rebuild foreground in the material-owning window, with one retained host per parent.
pub(crate) fn popover_lazy(
	id: impl Into<ElementId>,
	visible: bool,
	content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
) -> Popover {
	Popover {
		id: id.into(),
		visible,
		child: None,
		content: Some(std::rc::Rc::new(content)),
		dismiss: std::rc::Rc::new(|_| {}),
		trigger: Default::default(),
		above: false,
		preview: false,
		focus: None,
	}
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use crate::ui_motion::Tween;

	#[gpui::test]
	fn popover_keeps_its_final_size_during_the_whole_surface_arrival(
		cx: &mut gpui::TestAppContext,
	) {
		use gpui::{IntoElement, Render, Styled as _};

		struct Menu(bool);
		impl Render for Menu {
			fn render(
				&mut self,
				_: &mut gpui::Window,
				_: &mut gpui::Context<Self>,
			) -> impl IntoElement {
				super::popover("menu", self.0, gpui::div().h(gpui::px(48.)))
			}
		}

		let (menu, visual) = cx.add_window_view(|_, _| Menu(true));
		let mut size = None;
		for open in [true, false, true] {
			menu.update(visual, |menu, cx| {
				menu.0 = open;
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_frame);
			let bounds = visual.debug_bounds("popover-surface");
			assert_eq!(bounds.is_some(), open);
			if let Some(bounds) = bounds {
				assert!(bounds.top() >= gpui::px(crate::ui_theme::POPOVER_GAP));
				assert!(
					bounds.top()
						<= gpui::px(crate::ui_theme::POPOVER_GAP + crate::ui_theme::POPOVER_OFFSET)
				);
				assert_eq!(*size.get_or_insert(bounds.size), bounds.size);
			}
			visual.update(|window, cx| {
				assert_eq!(window.simulate_next_frame(cx) > 0, open);
			});
		}
	}

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
		let start = Instant::now();
		let mut tween = Tween::new(0.);

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
