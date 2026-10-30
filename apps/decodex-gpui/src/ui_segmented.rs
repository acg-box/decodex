//! One direct-manipulation control for Graph and Settings selections.
use std::rc::Rc;

use gpui::{
	App, Bounds, ClickEvent, DispatchPhase, Entity, IntoElement, KeyDownEvent, MouseButton,
	MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, RenderOnce, Role, SharedString, TextRun,
	Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{ui_motion, ui_theme};

type Change = Rc<dyn Fn(&usize, &mut Window, &mut App)>;
const INSET: f32 = 2.;

pub(crate) struct Choice {
	pub id: SharedString,
	pub label: &'static str,
	pub description: SharedString,
}
impl Choice {
	pub(crate) fn new(
		id: impl Into<SharedString>,
		label: &'static str,
		description: impl Into<SharedString>,
	) -> Self {
		Self { id: id.into(), label, description: description.into() }
	}
}

#[derive(IntoElement)]
pub(crate) struct Segmented {
	id: SharedString,
	label: SharedString,
	choices: Vec<Choice>,
	selected: usize,
	change: Change,
	graph: bool,
	thumb: SharedString,
}
impl Segmented {
	pub(crate) fn new(
		id: impl Into<SharedString>,
		label: impl Into<SharedString>,
		choices: impl IntoIterator<Item = Choice>,
		selected: usize,
		change: impl Fn(&usize, &mut Window, &mut App) + 'static,
	) -> Self {
		let id = id.into();
		let thumb = format!("{id}-thumb").into();
		Self {
			id,
			label: label.into(),
			choices: choices.into_iter().collect(),
			selected,
			change: Rc::new(change),
			graph: false,
			thumb,
		}
	}

	pub(crate) fn graph(mut self, thumb: &'static str) -> Self {
		self.graph = true;
		self.thumb = thumb.into();
		self
	}
}

#[derive(Default)]
struct State {
	bounds: Bounds<Pixels>,
	gesture: Option<Gesture>,
}
struct Gesture {
	start: f32,
	origin: f32,
	value: f32,
	moved: bool,
}
impl Gesture {
	fn move_to(&mut self, x: f32, width: f32, last: usize) {
		let delta = x - self.start;
		self.moved |= delta.abs() >= 3.;
		if self.moved {
			self.value = (self.origin + delta / width).clamp(0., last as f32);
		}
	}
}

impl RenderOnce for Segmented {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let Self { id, label, choices, selected, change, graph, thumb } = self;
		let last = choices.len().saturating_sub(1);
		let selected = selected.min(last);
		let height = if graph { ui_theme::CONTROL_GROUP_HEIGHT - 2. } else { 26. };
		let font_size = if graph { 12. } else { 11.5 };
		let width = if graph { 60. } else { choice_width(&choices, font_size, window) };
		let state =
			window.use_keyed_state(SharedString::from(format!("{id}-gesture")), cx, |_, _| {
				State::default()
			});
		let held = state.read(cx).gesture.is_some();
		let drag = state.read(cx).gesture.as_ref().filter(|g| g.moved);
		let value = drag.map_or(selected as f32, |g| g.value);
		let position = ui_motion::direct_value(thumb.clone(), value, drag.is_some(), window, cx);
		let down = state.clone();
		let mut control = if graph {
			ui_theme::canvas_control_group()
		} else {
			gpui::div().flex().flex_none().rounded(gpui::px(6.)).bg(ui_theme::rgba(0x00000020))
		};

		control = control
			.relative()
			.w(gpui::px(width * choices.len() as f32 + INSET * 2. + if graph { 2. } else { 0. }))
			.h(gpui::px(height + if graph { 2. } else { 0. }))
			.p(gpui::px(INSET))
			.gap_0();
		let mut control = control
			.id(id.clone())
			.debug_selector(move || id.to_string())
			.role(Role::RadioGroup)
			.aria_label(label)
			.occlude()
			.on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, _, cx| {
				down.update(cx, |s, cx| {
					let origin = ((f32::from(e.position.x - s.bounds.left()) - INSET) / width)
						.floor()
						.clamp(0., last as f32);
					s.gesture = Some(Gesture {
						start: e.position.x.into(),
						origin,
						value: origin,
						moved: false,
					});
					cx.notify();
				});
				cx.stop_propagation();
			})
			.child(
				gpui::div()
					.id(thumb.clone())
					.debug_selector(move || thumb.to_string())
					.absolute()
					.left(gpui::px(INSET + position * width))
					.top(gpui::px(INSET))
					.w(gpui::px(width))
					.h(gpui::px(height - INSET * 2.))
					.rounded(gpui::px(if graph { 8. } else { 4. }))
					.bg(ui_theme::rgba(ui_theme::CONTROL_SELECTED_FILL)),
			)
			.child(pointer_events(state.clone(), width, last, change.clone()));

		for (index, choice) in choices.into_iter().enumerate() {
			let key_change = change.clone();
			let click_change = change.clone();
			let key_state = state.clone();
			control = control.child(
				gpui::div()
					.id(choice.id.clone())
					.debug_selector(move || choice.id.to_string())
					.role(Role::RadioButton)
					.aria_label(choice.description)
					.aria_selected(selected == index)
					.tab_index(0)
					.relative()
					.w(gpui::px(width))
					.h(gpui::px(height - INSET * 2.))
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.rounded(gpui::px(if graph { 8. } else { 4. }))
					.border_1()
					.border_color(gpui::transparent_black())
					.text_size(gpui::px(font_size))
					.text_color(ui_theme::rgb(if selected == index {
						ui_theme::TEXT
					} else {
						ui_theme::TEXT_MUTED
					}))
					.cursor_pointer()
					.when(!held, |d| d.hover(|d| d.text_color(ui_theme::rgb(ui_theme::TEXT))))
					.focus_visible(|d| d.border_color(ui_theme::rgb(ui_theme::BLUE)))
					.child(choice.label)
					.on_click(move |event: &ClickEvent, window, cx| {
						if event.mouse_position().is_none() {
							click_change(&index, window, cx);
							cx.stop_propagation();
						}
					})
					.on_key_down(move |e: &KeyDownEvent, window, cx| {
						let next = match e.keystroke.key.as_str() {
							"escape" if key_state.read(cx).gesture.is_some() => {
								key_state.update(cx, |s, cx| {
									s.gesture = None;
									cx.notify();
								});
								selected
							},
							"left" => selected.saturating_sub(1),
							"right" => (selected + 1).min(last),
							"home" => 0,
							"end" => last,
							"enter" | "space" if !e.is_held => index,
							_ => return,
						};
						key_change(&next, window, cx);
						cx.stop_propagation();
					}),
			);
		}
		control
	}
}

fn choice_width(choices: &[Choice], font_size: f32, window: &Window) -> f32 {
	choices
		.iter()
		.map(|choice| {
			f32::from(
				window
					.text_system()
					.shape_line(
						choice.label.into(),
						gpui::px(font_size),
						&[TextRun {
							len: choice.label.len(),
							font: gpui::font(ui_theme::FONT_FAMILY),
							..Default::default()
						}],
						None,
					)
					.width,
			) + 16.
		})
		.fold(44., f32::max)
}

fn pointer_events(
	state: Entity<State>,
	width: f32,
	last: usize,
	change: Change,
) -> impl IntoElement {
	let measure = state.clone();
	gpui::canvas(
		move |bounds, _, cx| measure.update(cx, |s, _| s.bounds = bounds),
		move |_, _, window, _| {
			let movement = state.clone();
			let movement_change = change.clone();
			window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
				if phase != DispatchPhase::Capture || movement.read(cx).gesture.is_none() {
					return;
				}
				let next = movement.update(cx, |s, cx| {
					if e.pressed_button != Some(MouseButton::Left) {
						s.gesture = None;
						cx.notify();
						return None;
					}
					let gesture = s.gesture.as_mut()?;
					let previous = gesture.value.round() as usize;
					gesture.move_to(e.position.x.into(), width, last);
					let next = gesture.value.round() as usize;
					cx.notify();
					(gesture.moved && next != previous).then_some(next)
				});
				if let Some(next) = next {
					movement_change(&next, window, cx);
				}
				cx.stop_propagation();
			});
			let release = state.clone();
			let release_change = change.clone();
			window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
				if phase != DispatchPhase::Capture || e.button != MouseButton::Left {
					return;
				}
				let next = release.update(cx, |s, cx| {
					let mut gesture = s.gesture.take()?;
					gesture.move_to(e.position.x.into(), width, last);
					cx.notify();
					Some(gesture.value.round() as usize)
				});
				if let Some(next) = next {
					release_change(&next, window, cx);
					cx.stop_propagation();
				}
			});
		},
	)
	.absolute()
	.inset_0()
}
