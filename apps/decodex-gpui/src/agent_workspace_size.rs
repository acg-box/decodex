//! Content bounds and direct manipulation for the Agent workspace.

use gpui::{
	AnyElement, ClickEvent, Div, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
	Stateful,
};

use crate::{
	panel_preferences::PanelDefaults,
	shell::{
		WINDOW_CONTROLS_CLEARANCE,
		agent_surface::{
			AgentSurface, Context, InteractiveElement, IntoElement, ParentElement, Role,
			StatefulInteractiveElement, Styled, Window, graph,
		},
	},
	ui_motion,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Panel {
	Left,
	Right,
	Bottom,
}

impl AgentSurface {
	#[cfg(test)]
	pub(crate) fn panel_dimensions(&self) -> (f32, f32, f32) {
		(self.sidebar_width, self.agent_panel_width, self.graph_panel_height)
	}

	pub(crate) fn resize_panel(
		&mut self,
		delta: f32,
		reset: bool,
		all: bool,
		window: &Window,
		cx: &mut Context<Self>,
	) {
		let defaults = PanelDefaults::configured();
		let width = f32::from(window.viewport_size().width);
		let visible = [
			self.sidebar_visible && width > 1_000.0,
			self.agent_tree_visible && self.has_work() && !self.graph_expanded,
			self.graph_visible && self.has_work() && !self.graph_expanded,
		];

		for (index, panel) in [Panel::Left, Panel::Right, Panel::Bottom].into_iter().enumerate() {
			if !visible[index] || (!all && self.focused_panel != Some(panel)) {
				continue;
			}

			let (value, default, min, max) = match panel {
				Panel::Left => (&mut self.sidebar_width, defaults.sidebar, 160.0, 480.0),
				Panel::Right => (&mut self.agent_panel_width, defaults.sidebar, 160.0, 480.0),
				Panel::Bottom => (&mut self.graph_panel_height, defaults.dock, 120.0, 640.0),
			};

			*value = if reset { f32::from(default) } else { (*value + delta).clamp(min, max) };
		}

		cx.notify();
	}

	pub(super) fn update_graph_inset(&mut self, width: f32, height: f32) {
		let layout = self.workspace_graph_layout();
		let (right, bottom) = layout
			.nodes
			.iter()
			.fold((0.0_f32, 0.0_f32), |(x, y), node| (x.max(node.x + 160.0), y.max(node.y + 52.0)));
		let zoom = self.graph_display_zoom;

		self.graph_inset = (
			((width - (right + 20.0) * zoom) / 2.0).max(0.0),
			if self.graph_expanded {
				((height - 118.0 - (bottom + 32.0) * zoom) / 2.0).max(0.0)
			} else {
				0.0
			},
		);
	}

	pub(super) fn workspace_graph_layout(&self) -> graph::Layout {
		let Some(snapshot) = &self.snapshot else {
			return graph::Layout::default();
		};
		let scope = self.graph_scope.clone().or_else(|| self.root_id());
		let mut layout = graph::Layout::new(snapshot, scope.as_deref());

		if layout.nodes.is_empty()
			&& let Some(work) = snapshot.work_items.iter().find(|w| Some(&w.id) == scope.as_ref())
		{
			layout = graph::Layout::new(snapshot, work.parent_goal_id.as_deref());
		}

		for node in &mut layout.nodes {
			let (x, y) = (node.x, node.y);

			node.x = 32.0 + (y - 32.0) / 112.0 * 212.0;
			node.y = 20.0 + (x - 20.0) / 188.0 * 92.0;
		}

		layout
	}

	pub(super) fn workspace_graph_size(&self, window: &Window, wide: bool) -> (f32, f32) {
		if !self.graph_visible || !self.reserve_workspace_panels() {
			return (0.0, 0.0);
		}

		let viewport = window.viewport_size();
		let sidebar = if self.sidebar_visible && wide {
			sidebar_width(self.sidebar_width, viewport.width.into())
		} else {
			0.0
		};
		let available = (
			f32::from(viewport.width) - sidebar - self.agent_tree_width(window),
			(f32::from(viewport.height) - WINDOW_CONTROLS_CLEARANCE).max(0.0),
		);

		if !self.graph_expanded {
			return (
				available.0,
				self.graph_panel_height.clamp(120.0, 640.0).min((available.1 - 240.0).max(0.0)),
			);
		}

		available
	}

	pub(super) fn sidebar_slot(
		&self,
		wide: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let width = sidebar_width(self.sidebar_width, window.viewport_size().width.into());
		let fraction = ui_motion::value(
			"agent-sidebar-visibility",
			if self.sidebar_visible && wide { 1.0 } else { 0.0 },
			window,
			cx,
		);

		gpui::div()
			.flex_none()
			.w(gpui::px(width * fraction))
			.h_full()
			.overflow_hidden()
			.id("left-panel-slot")
			.capture_any_mouse_down(cx.listener(|s, _, _, _| s.focused_panel = Some(Panel::Left)))
			.child(gpui::div().w(gpui::px(width)).h_full().child(self.workspace_sidebar(cx)))
			.into_any_element()
	}

	pub(super) fn sidebar_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.id("agent-sidebar-resize")
			.absolute()
			.right_0()
			.top_0()
			.bottom_0()
			.w(gpui::px(6.0))
			.cursor_col_resize()
			.tab_index(0)
			.role(Role::Slider)
			.aria_label("Sidebar width. Drag or use Left and Right. Double-click to reset.")
			.hover(|s| s.bg(gpui::rgba(0xffffff18)))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(|s, event: &MouseDownEvent, window, cx| {
					s.sidebar_drag = Some((
						event.position.x.into(),
						sidebar_width(s.sidebar_width, window.viewport_size().width.into()),
					));

					cx.stop_propagation();
				}),
			)
			.on_click(cx.listener(|s, event: &ClickEvent, _, cx| {
				if event.click_count() == 2 {
					s.sidebar_width = PanelDefaults::configured().sidebar.into();

					cx.notify();
				}
			}))
			.on_key_down(cx.listener(|s, event: &KeyDownEvent, window, cx| {
				let delta = match event.keystroke.key.as_str() {
					"left" => -16.0,
					"right" => 16.0,
					_ => return,
				};

				s.sidebar_width =
					sidebar_width(s.sidebar_width + delta, window.viewport_size().width.into());

				cx.stop_propagation();
				cx.notify();
			}))
	}

	pub(super) fn workspace_resize_root(&self, cx: &mut Context<Self>) -> Stateful<Div> {
		gpui::div()
			.id("agent-workspace")
			.on_mouse_move(cx.listener(|s, event: &MouseMoveEvent, window, cx| {
				let Some((start, width)) = s.sidebar_drag else {
					return;
				};

				if event.pressed_button != Some(MouseButton::Left) {
					s.sidebar_drag = None;

					return;
				}

				s.sidebar_width = sidebar_width(
					width + f32::from(event.position.x) - start,
					window.viewport_size().width.into(),
				);

				cx.stop_propagation();
				cx.notify();
			}))
			.on_mouse_up(
				MouseButton::Left,
				cx.listener(|s, _, _, _| {
					s.sidebar_drag = None;
				}),
			)
			.on_mouse_up_out(
				MouseButton::Left,
				cx.listener(|s, _, _, _| {
					s.sidebar_drag = None;
				}),
			)
	}
}

fn sidebar_width(requested: f32, viewport: f32) -> f32 {
	requested.clamp(160.0, (viewport - 600.0).clamp(160.0, 480.0))
}

#[cfg(test)]
mod tests {

	use crate::shell::agent_surface::workspace_size::{self, AgentSurface, MouseButton, Panel};
	#[gpui::test]
	fn sidebar_drag_tracks_pointer_and_stops_on_release(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.0), gpui::px(900.0)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.sidebar_width = 192.0;
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let point = |x| gpui::point(gpui::px(x), gpui::px(300.0));

		visual.simulate_mouse_down(point(189.0), MouseButton::Left, Default::default());
		visual.simulate_mouse_move(point(269.0), MouseButton::Left, Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.sidebar_width, 272.0);
			assert_eq!(s.graph_pan, (0.0, 0.0));
		});
		visual.simulate_mouse_up(point(269.0), MouseButton::Left, Default::default());
		visual.simulate_mouse_move(point(400.0), None, Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.sidebar_width, 272.0);
			assert!(s.sidebar_drag.is_none());
		});
	}

	#[gpui::test]
	fn panel_shortcuts_resize_only_the_selected_visible_panels(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.0), gpui::px(900.0)));

		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.sidebar_width = 240.0;
				s.agent_panel_width = 240.0;
				s.graph_panel_height = 240.0;
				s.agent_tree_visible = true;
				s.graph_visible = true;
				s.graph_expanded = false;
				s.focused_panel = Some(Panel::Right);

				s.resize_panel(24.0, false, false, window, cx);

				assert_eq!(
					(s.sidebar_width, s.agent_panel_width, s.graph_panel_height),
					(240.0, 264.0, 240.0)
				);

				s.sidebar_visible = false;

				s.resize_panel(-24.0, false, true, window, cx);

				assert_eq!(
					(s.sidebar_width, s.agent_panel_width, s.graph_panel_height),
					(240.0, 240.0, 216.0)
				);

				s.resize_panel(0.0, true, true, window, cx);

				assert_eq!(
					s.graph_panel_height,
					f32::from(crate::panel_preferences::PanelDefaults::configured().dock)
				);

				s.focused_panel = None;

				s.resize_panel(24.0, false, false, window, cx);

				assert_eq!(
					s.agent_panel_width,
					f32::from(crate::panel_preferences::PanelDefaults::configured().sidebar)
				);
			})
		});
	}

	#[gpui::test]
	fn graph_panel_respects_manual_height_and_available_space(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_200.), gpui::px(900.)));

		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.sidebar_visible = false;
				s.agent_tree_visible = false;
				s.graph_visible = true;
				s.graph_expanded = false;
				s.graph_panel_height = 275.;

				assert_eq!(s.workspace_graph_size(window, true), (1_200., 275.));

				s.graph_zoom = 1.8;
				s.graph_pan = (800., 600.);

				assert_eq!(s.workspace_graph_size(window, true), (1_200., 275.));

				s.graph_expanded = true;

				assert_eq!(
					s.workspace_graph_size(window, true),
					(1_200., 900. - super::super::super::WINDOW_CONTROLS_CLEARANCE)
				);

				s.graph_visible = false;

				assert_eq!(s.workspace_graph_size(window, true), (0., 0.));

				s.graph_visible = true;
				s.graph_expanded = false;
			});
		});

		visual.simulate_resize(gpui::size(gpui::px(1_200.), gpui::px(300.)));

		visual.update(|window, cx| {
			surface.update(cx, |s, _| {
				let (_, height) = s.workspace_graph_size(window, true);

				assert_eq!(
					height,
					(300. - super::super::super::WINDOW_CONTROLS_CLEARANCE - 240.).max(0.)
				);
				assert_eq!(s.graph_panel_height, 275., "small windows retain the requested height");
			});
		});
	}
	#[test]
	fn sidebar_limits_preserve_main_space() {
		assert_eq!(workspace_size::sidebar_width(80.0, 1_200.0), 160.0);
		assert_eq!(workspace_size::sidebar_width(500.0, 1_200.0), 480.0);
		assert_eq!(workspace_size::sidebar_width(300.0, 800.0), 200.0);
		assert_eq!(workspace_size::sidebar_width(256.0, 1_200.0), 256.0);
	}
}
