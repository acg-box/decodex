//! Compact switches share one click, drag and keyboard interaction.
use std::rc::Rc;

use gpui::{
	App, Bounds, ClickEvent, DispatchPhase, Div, Entity, IntoElement, KeyDownEvent, MouseButton,
	MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderOnce, Rgba, Role,
	SharedString, Stateful, Window,
	accesskit::Toggled,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	ui_motion,
	ui_theme::{
		self, ACCENT, CONTROL_ACTIVE_FILL, LINE_STRONG, ON_PRIMARY, SETTINGS_CONTROL_HEIGHT,
		SETTINGS_CONTROL_RADIUS, TEXT, appearance::Theme,
	},
};

type Change = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

const TRAVEL: f32 = 12.;

#[derive(IntoElement)]
pub(crate) struct Switch {
	id: SharedString,
	knob: SharedString,
	label: SharedString,
	enabled: bool,
	interactive: bool,
	description: Option<&'static str>,
	change: Option<Change>,
}
impl Switch {
	pub(crate) fn on_change(
		mut self,
		change: impl Fn(&bool, &mut Window, &mut App) + 'static,
	) -> Self {
		self.change = Some(Rc::new(change));

		self
	}

	pub(crate) fn aria_description(mut self, description: &'static str) -> Self {
		self.description = Some(description);

		self
	}
}

impl RenderOnce for Switch {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let Self { id, knob, label, enabled, interactive, description, change } = self;
		let state =
			window.use_keyed_state(SharedString::from(format!("{id}-gesture")), cx, |_, _| {
				State::default()
			});

		if !interactive {
			state.update(cx, |s, _| s.gesture = None);
		}

		let dragging = state.read(cx).gesture.as_ref().is_some_and(|g| g.dragging);
		let target = state
			.read(cx)
			.gesture
			.as_ref()
			.filter(|g| g.dragging)
			.map_or(f32::from(enabled), |g| g.position);
		let position = ui_motion::direct_value(knob.clone(), target, dragging, window, cx);
		let thumb = if Theme::current() == Theme::Dark {
			ui_theme::rgb(TEXT).blend(ui_theme::rgb(ON_PRIMARY).opacity(position))
		} else {
			gpui::rgb(0xffffff)
		};
		let down = state.clone();
		let key_state = state.clone();
		let key_change = change.clone();
		let track = SharedString::from(format!("{id}-track"));

		gpui::div()
			.id(id.clone())
			.debug_selector(move || id.to_string())
			.role(Role::Switch)
			.aria_label(label)
			.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
			.when_some(description, |d, description| d.aria_description(description))
			.tab_index(if interactive { 0 } else { -1 })
			.w(gpui::px(34.))
			.h(gpui::px(SETTINGS_CONTROL_HEIGHT))
			.flex_none()
			.relative()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(SETTINGS_CONTROL_RADIUS))
			.border_1()
			.border_color(ui_theme::rgba(0))
			.opacity(if interactive { 1. } else { 0.45 })
			.when(interactive, |d| d.cursor_pointer().hover(|d| d.opacity(0.86)))
			.focus_visible(|d| d.border_color(ui_theme::rgb(ACCENT)))
			.when(interactive, |d| {
				d.on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
					down.update(cx, |s, cx| {
						s.gesture = Some(Gesture {
							origin: event.position,
							initial: enabled,
							position: f32::from(enabled),
							dragging: false,
						});

						cx.notify();
					});
				})
				.on_key_down(move |event: &KeyDownEvent, window, cx| {
					if event.keystroke.key == "escape" && key_state.read(cx).gesture.is_some() {
						key_state.update(cx, |s, cx| {
							s.gesture = None;

							cx.notify();
						});

						cx.stop_propagation();
					} else if !event.is_held
						&& ["enter", "space"].contains(&event.keystroke.key.as_str())
					{
						if let Some(change) = &key_change {
							change(&!enabled, window, cx);
						}

						cx.stop_propagation();
					}
				})
				.on_click({
					let change = change.clone();

					move |event, window, cx| {
						if matches!(event, ClickEvent::Keyboard(_) | ClickEvent::Touch(_)) {
							if let Some(change) = &change {
								change(&!enabled, window, cx);
							}

							cx.stop_propagation();
						}
					}
				})
			})
			.child(switch_track(track, knob, position, thumb))
			.child(pointer_events(state, enabled, interactive, change))
	}
}

struct Gesture {
	origin: Point<Pixels>,
	initial: bool,
	position: f32,
	dragging: bool,
}
impl Gesture {
	fn move_to(&mut self, pointer: Point<Pixels>) {
		let delta = f32::from(pointer.x - self.origin.x);

		self.dragging |= delta.abs() >= 3.;
		self.position = (f32::from(self.initial) + delta / TRAVEL).clamp(0., 1.);
	}

	fn finish(mut self, pointer: Point<Pixels>, bounds: Bounds<Pixels>) -> Option<bool> {
		self.move_to(pointer);

		if self.dragging {
			Some(self.position >= 0.5)
		} else {
			bounds.contains(&pointer).then_some(!self.initial)
		}
	}
}

#[derive(Default)]
struct State {
	gesture: Option<Gesture>,
	bounds: Bounds<Pixels>,
}

pub(crate) fn settings_switch(
	id: impl Into<SharedString>,
	knob: impl Into<SharedString>,
	label: impl Into<SharedString>,
	enabled: bool,
	interactive: bool,
) -> Switch {
	Switch {
		id: id.into(),
		knob: knob.into(),
		label: label.into(),
		enabled,
		interactive,
		description: None,
		change: None,
	}
}

fn switch_track(
	track: SharedString,
	knob: SharedString,
	position: f32,
	thumb: Rgba,
) -> Stateful<Div> {
	gpui::div()
		.id(track.clone())
		.debug_selector(move || track.to_string())
		.w(gpui::px(30.))
		.h(gpui::px(18.))
		.flex_none()
		.p(gpui::px(2.))
		.flex()
		.items_center()
		.rounded_full()
		.bg(ui_theme::rgb(LINE_STRONG).blend(ui_theme::rgb(CONTROL_ACTIVE_FILL).opacity(position)))
		.child(
			gpui::div()
				.ml(gpui::px(position * TRAVEL))
				.size(gpui::px(14.))
				.flex_none()
				.id(knob.clone())
				.debug_selector(move || knob.to_string())
				.rounded_full()
				.bg(thumb),
		)
}

fn pointer_events(
	state: Entity<State>,
	enabled: bool,
	interactive: bool,
	change: Option<Change>,
) -> impl IntoElement {
	let measure = state.clone();

	gpui::canvas(
		move |bounds, _, cx| {
			measure.update(cx, |s, _| s.bounds = bounds);
		},
		move |_, _, window, _| {
			if !interactive {
				return;
			}

			let movement = state.clone();

			window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
				if phase != DispatchPhase::Capture || movement.read(cx).gesture.is_none() {
					return;
				}

				movement.update(cx, |s, cx| {
					if event.pressed_button == Some(MouseButton::Left) {
						if let Some(gesture) = &mut s.gesture {
							gesture.move_to(event.position);
						}
					} else {
						s.gesture = None;
					}

					cx.notify();
				});

				cx.stop_propagation();
			});

			let release = state.clone();
			let change = change.clone();

			window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
				if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
					return;
				}

				let result = release.update(cx, |s, cx| {
					let gesture = s.gesture.take()?;

					cx.notify();

					Some(gesture.finish(event.position, s.bounds))
				});

				if let Some(result) = result {
					if let Some(desired) = result.filter(|desired| *desired != enabled)
						&& let Some(change) = &change
					{
						change(&desired, window, cx);
					}

					cx.stop_propagation();
				}
			});
		},
	)
	.absolute()
	.inset_0()
}

#[cfg(test)]
mod tests {
	use gpui::{Context, Render, TestAppContext, VisualTestContext};

	use crate::settings_surface::switch::{
		self, IntoElement, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TRAVEL, Window,
	};

	struct Harness {
		enabled: bool,
		interactive: bool,
		changes: usize,
	}
	impl Render for Harness {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			switch::settings_switch(
				"test-switch",
				"test-thumb",
				"Test switch",
				self.enabled,
				self.interactive,
			)
			.on_change(cx.listener(|s, desired: &bool, _, cx| {
				s.enabled = *desired;
				s.changes += 1;

				cx.notify();
			}))
		}
	}

	fn press(visual: &mut VisualTestContext, point: Point<Pixels>) {
		visual.simulate_event(MouseDownEvent {
			position: point,
			button: MouseButton::Left,
			..Default::default()
		});
	}

	fn release(visual: &mut VisualTestContext, point: Point<Pixels>) {
		visual.simulate_event(MouseUpEvent {
			position: point,
			button: MouseButton::Left,
			..Default::default()
		});
		visual.update(crate::ui_frame::draw_current_frame);
	}

	#[gpui::test]
	fn settings_switch_click_drag_and_keyboard_commit_once(cx: &mut TestAppContext) {
		let (switch, visual) =
			cx.add_window_view(|_, _| Harness { enabled: false, interactive: true, changes: 0 });

		visual.update(crate::ui_frame::draw_current_frame);

		let bounds = visual.debug_bounds("test-switch").unwrap();

		visual.simulate_click(bounds.center(), Default::default());

		assert_eq!(switch.read_with(visual, |s, _| (s.enabled, s.changes)), (true, 1));

		visual.update(crate::ui_frame::draw_current_frame);

		let start = bounds.center();
		let end = start - gpui::point(gpui::px(TRAVEL), gpui::px(0.));

		press(visual, start);

		visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
		visual.update(crate::ui_frame::draw_current_frame);

		assert_eq!(
			switch.read_with(visual, |s, _| (s.enabled, s.changes)),
			(true, 1),
			"dragging only previews"
		);

		let thumb = visual.debug_bounds("test-thumb").unwrap();
		let track = visual.debug_bounds("test-switch-track").unwrap();

		assert_eq!(thumb.left(), track.left() + gpui::px(2.));

		release(visual, end);

		assert_eq!(switch.read_with(visual, |s, _| (s.enabled, s.changes)), (false, 2));

		visual.simulate_keystrokes("space");

		assert_eq!(switch.read_with(visual, |s, _| (s.enabled, s.changes)), (true, 3));

		visual.simulate_keystrokes("enter");

		assert_eq!(switch.read_with(visual, |s, _| (s.enabled, s.changes)), (false, 4));
	}

	#[gpui::test]
	fn settings_switch_drag_can_return_and_release_outside(cx: &mut TestAppContext) {
		let (switch, visual) =
			cx.add_window_view(|_, _| Harness { enabled: false, interactive: true, changes: 0 });

		visual.update(crate::ui_frame::draw_current_frame);

		let start = visual.debug_bounds("test-switch").unwrap().center();
		let right = start + gpui::point(gpui::px(60.), gpui::px(0.));

		press(visual, start);

		visual.simulate_mouse_move(right, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(start, MouseButton::Left, Default::default());

		release(visual, start);

		assert_eq!(switch.read_with(visual, |s, _| (s.enabled, s.changes)), (false, 0));

		press(visual, start);

		visual.simulate_mouse_move(right, MouseButton::Left, Default::default());

		release(visual, right);

		assert_eq!(switch.read_with(visual, |s, _| (s.enabled, s.changes)), (true, 1));

		press(visual, start);

		let below = start + gpui::point(gpui::px(0.), gpui::px(80.));

		visual.simulate_mouse_move(below, MouseButton::Left, Default::default());

		release(visual, below);

		assert_eq!(
			switch.read_with(visual, |s, _| (s.enabled, s.changes)),
			(true, 1),
			"an outside click cancels"
		);

		press(visual, start);

		let left = start - gpui::point(gpui::px(TRAVEL), gpui::px(0.));

		visual.simulate_mouse_move(left, MouseButton::Left, Default::default());
		visual.simulate_keystrokes("escape");

		release(visual, left);

		assert_eq!(switch.read_with(visual, |s, _| (s.enabled, s.changes)), (true, 1));
	}

	#[gpui::test]
	fn settings_switch_disabled_ignores_clicks_and_drags(cx: &mut TestAppContext) {
		let (switch, visual) =
			cx.add_window_view(|_, _| Harness { enabled: false, interactive: false, changes: 0 });

		visual.update(crate::ui_frame::draw_current_frame);

		let start = visual.debug_bounds("test-switch").unwrap().center();

		visual.simulate_click(start, Default::default());

		press(visual, start);

		let end = start + gpui::point(gpui::px(20.), gpui::px(0.));

		visual.simulate_mouse_move(end, MouseButton::Left, Default::default());

		release(visual, end);

		assert_eq!(switch.read_with(visual, |s, _| (s.enabled, s.changes)), (false, 0));
	}
}
