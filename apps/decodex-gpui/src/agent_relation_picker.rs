//! Sliding layout segments. Direct manipulation shares the graph's existing layout transition.
use super::{
	AgentSurface, AnyElement, Context, FluentBuilder as _, InteractiveElement as _, IntoElement,
	MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement as _, Role, SharedString,
	SmoothControl as _, StatefulInteractiveElement as _, Styled as _, TEXT, TEXT_MUTED, layout,
	restore_shell_focus,
};
use crate::{ui_motion, ui_theme};

const SEGMENT: f32 = 60.;
const INSET: f32 = 2.;

pub(super) struct Drag {
	start: f32,
	origin: f32,
	value: f32,
	moved: bool,
	clicked: layout::Mode,
}
impl Drag {
	fn new(x: f32, left: f32) -> Self {
		let index = ((x - left - INSET) / SEGMENT).floor().clamp(0., 2.);
		Self {
			start: x,
			origin: index,
			value: index,
			moved: false,
			clicked: layout::Mode::ALL[index as usize],
		}
	}

	fn move_to(&mut self, x: f32) {
		let delta = x - self.start;
		self.moved |= delta.abs() >= 3.;
		if self.moved {
			self.value = (self.origin + delta / SEGMENT).clamp(0., 2.);
		}
	}

	fn mode(&self) -> layout::Mode {
		if self.moved { layout::Mode::ALL[self.value.round() as usize] } else { self.clicked }
	}
}

impl AgentSurface {
	pub(super) fn relation_layout_picker(&self, cx: &mut Context<Self>) -> AnyElement {
		let mode = self.work_board.view.layout_mode;
		let index = layout::Mode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
		let held = self.work_board.view.layout_drag.is_some();
		let dragging = self.work_board.view.layout_drag.as_ref().filter(|drag| drag.moved);
		let bounds = std::rc::Rc::new(std::cell::Cell::new(None::<gpui::Bounds<gpui::Pixels>>));
		let measured = bounds.clone();
		let mut picker = ui_theme::canvas_control_group()
			.id("relations-layout-picker")
			.debug_selector(|| "relations-layout-picker".into())
			.role(Role::Group)
			.aria_label("Graph layout. Click a layout or drag between layouts.")
			.relative()
			.w(gpui::px(SEGMENT * 3. + (INSET + 1.) * 2.))
			.p(gpui::px(INSET))
			.gap_0()
			.occlude()
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(move |s, event: &MouseDownEvent, _, cx| {
					if let Some(bounds) = bounds.get() {
						s.work_board.view.layout_drag =
							Some(Drag::new(event.position.x.into(), bounds.left().into()));
						s.work_board.view.drag = None;
						s.work_board.view.map_drag = None;
						cx.stop_propagation();
						cx.notify();
					}
				}),
			)
			.child(picker_events(measured, cx.entity().downgrade()))
			.child(Thumb {
				value: dragging.map_or(index as f32, |d| d.value),
				dragging: dragging.is_some(),
			});
		for mode in layout::Mode::ALL {
			let active = self.work_board.view.layout_mode == mode;
			let id = format!("relations-layout-{}", mode.label().to_lowercase());
			picker = picker.child(
				gpui::div()
					.id(SharedString::from(id.clone()))
					.debug_selector(move || id.clone())
					.role(Role::Button)
					.aria_label(format!("{} layout", mode.label()))
					.aria_selected(active)
					.tab_index(0)
					.relative()
					.w(gpui::px(SEGMENT))
					.h_full()
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.rounded(gpui::px(5.))
					.border_1()
					.border_color(gpui::transparent_black())
					.text_color(gpui::rgb(if active { TEXT } else { TEXT_MUTED }))
					.cursor_pointer()
					.when(!held, |d| d.hover(|d| d.text_color(gpui::rgb(TEXT))))
					.focus_visible(|d| d.border_color(gpui::rgb(ui_theme::BLUE)))
					.on_click(cx.listener(move |s, event: &gpui::ClickEvent, window, cx| {
						// Pointer clicks are committed by the captured release, including outside
						// the track.
						if event.mouse_position().is_none() {
							s.switch_relation_layout(mode, cx);
							restore_shell_focus(window, cx);
						}
					}))
					.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
						let current = layout::Mode::ALL
							.iter()
							.position(|m| *m == s.work_board.view.layout_mode)
							.unwrap_or(0);
						let next = match event.keystroke.key.as_str() {
							"left" => current.saturating_sub(1),
							"right" => (current + 1).min(2),
							"home" => 0,
							"end" => 2,
							"enter" | "space" if !event.is_held => {
								s.switch_relation_layout(mode, cx);
								restore_shell_focus(window, cx);
								cx.stop_propagation();
								return;
							},
							_ => return,
						};
						s.switch_relation_layout(layout::Mode::ALL[next], cx);
						cx.stop_propagation();
					}))
					.child(mode.label()),
			);
		}
		picker.smooth().into_any_element()
	}

	fn finish_layout_drag(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
		if let Some(drag) = self.work_board.view.layout_drag.take() {
			self.switch_relation_layout(drag.mode(), cx);
			restore_shell_focus(window, cx);
			cx.notify();
		}
	}
}

fn picker_events(
	measured: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
	events: gpui::WeakEntity<AgentSurface>,
) -> impl IntoElement {
	gpui::canvas(
		move |bounds, _, _| measured.set(Some(bounds)),
		move |_, _, window, _| {
			let movement = events.clone();
			window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
				if phase != gpui::DispatchPhase::Capture {
					return;
				}
				let _ = movement.update(cx, |s, cx| {
					let Some(drag) = &mut s.work_board.view.layout_drag else { return };
					if event.pressed_button == Some(MouseButton::Left) {
						drag.move_to(event.position.x.into());
						if drag.moved {
							let mode = drag.mode();
							s.switch_relation_layout(mode, cx);
							cx.notify();
						}
					} else {
						s.finish_layout_drag(window, cx);
					}
					cx.stop_propagation();
				});
			});
			let release = events.clone();
			window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, window, cx| {
				if phase == gpui::DispatchPhase::Capture && event.button == MouseButton::Left {
					let _ = release.update(cx, |s, cx| {
						if s.work_board.view.layout_drag.is_some() {
							s.finish_layout_drag(window, cx);
						}
					});
				}
			});
		},
	)
	.absolute()
	.inset_0()
}

#[derive(gpui::IntoElement)]
struct Thumb {
	value: f32,
	dragging: bool,
}
impl gpui::RenderOnce for Thumb {
	fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
		let value =
			ui_motion::direct_value("graph-layout-thumb", self.value, self.dragging, window, cx);
		gpui::div()
			.id("relations-layout-thumb")
			.debug_selector(|| "relations-layout-thumb".into())
			.absolute()
			.left(gpui::px(INSET + value * SEGMENT))
			.top(gpui::px(INSET))
			.w(gpui::px(SEGMENT))
			.h(gpui::px(ui_theme::CONTROL_GROUP_HEIGHT - (INSET + 1.) * 2.))
			.rounded(gpui::px(5.))
			.bg(gpui::rgba(ui_theme::PRESSED_FILL))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fixture(cx: &mut Context<AgentSurface>) -> AgentSurface {
		let mut s = super::super::super::tests::fixture(cx);
		s.visual_workspace_page("workbench", cx);
		s.workspace.graph_expanded = true;
		s.work_board.view.camera_fixed = true;
		s.work_board.view.zoom = Some(0.7);
		s.work_board.view.picked_agent = Some("release".into());
		s.composer.update(cx, |input, cx| input.set_content("Preserve the Main draft", cx));
		s
	}

	#[gpui::test]
	fn layout_picker_clicks_slide_to_each_mode_without_changing_the_chat(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		let main = surface
			.read_with(visual, |s, _| (s.selected.clone(), s.native_agents.selected.clone()));
		for (mode, selector) in [
			(layout::Mode::Radial, "relations-layout-radial"),
			(layout::Mode::Grouped, "relations-layout-grouped"),
			(layout::Mode::Tree, "relations-layout-tree"),
		] {
			let button = visual.debug_bounds(selector).unwrap();
			visual.simulate_click(button.center(), Default::default());
			visual.update(crate::ui_frame::draw_frame);
			surface.read_with(visual, |s, cx| {
				assert_eq!(s.work_board.view.layout_mode, mode);
				assert!(s.work_board.view.layout_drag.is_none());
				assert_eq!(s.work_board.view.zoom, Some(0.7));
				assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
				assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), main);
				assert_eq!(s.composer.read(cx).content(), "Preserve the Main draft");
			});
			std::thread::sleep(std::time::Duration::from_millis(230));
			visual.update(crate::ui_frame::draw_frame);
			assert_eq!(visual.debug_bounds("relations-layout-thumb").unwrap(), button);
		}
	}

	#[gpui::test]
	fn layout_picker_drag_tracks_the_grab_point_and_releases_outside_the_track(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		let tree = visual.debug_bounds("relations-layout-tree").unwrap();
		let start = tree.center() + gpui::point(gpui::px(12.), gpui::px(0.));
		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		for (delta, mode) in [
			(24., layout::Mode::Tree),
			(72., layout::Mode::Radial),
			(160., layout::Mode::Grouped),
			(-80., layout::Mode::Tree),
			(60., layout::Mode::Radial),
		] {
			visual.simulate_mouse_move(
				start + gpui::point(gpui::px(delta), gpui::px(32.)),
				Some(MouseButton::Left),
				Default::default(),
			);
			visual.update(crate::ui_frame::draw_frame);
			surface.read_with(visual, |s, _| {
				assert_eq!(s.work_board.view.layout_mode, mode);
				assert!(s.work_board.view.drag.is_none(), "picker drags cannot pan the canvas");
				assert!(s.work_board.view.map_drag.is_none());
			});
			let thumb = visual.debug_bounds("relations-layout-thumb").unwrap();
			assert!((f32::from(thumb.left() - tree.left()) - delta.clamp(0., 120.)).abs() < 0.1);
		}
		let canvas = visual.debug_bounds("relations-canvas").unwrap();
		let outside = gpui::point(start.x + gpui::px(60.), canvas.bottom() - gpui::px(10.));
		visual.simulate_mouse_up(outside, MouseButton::Left, Default::default());
		visual.update(crate::ui_frame::draw_frame);
		let pan = surface.read_with(visual, |s, _| {
			assert!(s.work_board.view.layout_drag.is_none());
			assert_eq!(s.work_board.view.layout_mode, layout::Mode::Radial);
			assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
			assert_eq!(s.work_board.view.zoom, Some(0.7));
			s.work_board.view.pan
		});
		visual.simulate_mouse_move(tree.center(), None, Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.work_board.view.layout_mode, layout::Mode::Radial);
			assert_eq!(s.work_board.view.pan, pan);
		});
	}

	#[gpui::test]
	fn layout_picker_supports_keyboard_selection(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		let tree = visual.debug_bounds("relations-layout-tree").unwrap();
		visual.simulate_mouse_down(tree.center(), MouseButton::Left, Default::default());
		let focus = visual.update(|window, cx| window.focused(cx).unwrap());
		visual.simulate_mouse_up(tree.center(), MouseButton::Left, Default::default());
		visual.update(|window, cx| window.focus(&focus, cx));
		for (key, mode) in [
			("right", layout::Mode::Radial),
			("end", layout::Mode::Grouped),
			("left", layout::Mode::Radial),
			("home", layout::Mode::Tree),
		] {
			visual.simulate_keystrokes(key);
			visual.update(crate::ui_frame::draw_frame);
			surface.read_with(visual, |s, _| assert_eq!(s.work_board.view.layout_mode, mode));
		}
	}
}
