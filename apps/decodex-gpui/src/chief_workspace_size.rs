//! Content bounds and direct manipulation for the Chief workspace.
use super::*;
use gpui::{AnyElement, MouseButton, MouseMoveEvent};

fn sidebar_width(requested: f32, viewport: f32) -> f32 {
	requested.clamp(160.0, (viewport - 600.0).clamp(160.0, 360.0))
}

fn graph_size(
	layout: &graph::Layout,
	zoom: f32,
	available: (f32, f32),
	expanded: bool,
) -> (f32, f32) {
	if expanded {
		return available;
	}
	let bottom = layout.nodes.iter().map(|node| node.y + 52.0).fold(0.0_f32, f32::max);
	let annotations =
		if layout.edges.is_empty() { 0.0 } else { 16.0 } + if layout.cyclic { 18.0 } else { 0.0 };
	// Pan changes the camera, never the panel bounds. Keep room for the conversation.
	let height = (bottom * zoom + 24.0 + 84.0 + annotations)
		.clamp(180.0, 360.0)
		.min(available.1 * 0.45)
		.min((available.1 - 240.0).max(0.0));
	(available.0, height)
}

impl ChiefSurface {
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
		if !self.graph_visible || !self.has_work() {
			return (0.0, 0.0);
		}
		let viewport = window.viewport_size();
		let sidebar = if self.sidebar_visible && wide {
			sidebar_width(self.sidebar_width, viewport.width.into())
		} else {
			0.0
		};
		let tabs = if self.pages.is_empty() { 0.0 } else { 35.0 };

		graph_size(
			&self.workspace_graph_layout(),
			self.graph_zoom,
			(
				f32::from(viewport.width) - sidebar - self.agent_tree_width(window),
				(f32::from(viewport.height) - super::super::WINDOW_CONTROLS_CLEARANCE - tabs)
					.max(0.0),
			),
			self.graph_expanded,
		)
	}

	pub(super) fn sidebar_slot(
		&self,
		wide: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let width = sidebar_width(self.sidebar_width, window.viewport_size().width.into());
		let fraction = crate::ui_motion::value(
			"chief-sidebar-visibility",
			if self.sidebar_visible && wide { 1.0 } else { 0.0 },
			window,
			cx,
		);
		div()
			.flex_none()
			.w(px(width * fraction))
			.h_full()
			.overflow_hidden()
			.child(div().w(px(width)).h_full().child(self.workspace_sidebar(cx)))
			.into_any_element()
	}

	pub(super) fn sidebar_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.id("chief-sidebar-resize")
			.absolute()
			.right_0()
			.top_0()
			.bottom_0()
			.w(px(6.0))
			.cursor_col_resize()
			.tab_index(0)
			.role(Role::Slider)
			.aria_label("Sidebar width. Drag or use Left and Right. Double-click to reset.")
			.hover(|s| s.bg(rgba(0xffffff18)))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(|s, event: &gpui::MouseDownEvent, window, cx| {
					s.sidebar_drag = Some((
						event.position.x.into(),
						sidebar_width(s.sidebar_width, window.viewport_size().width.into()),
					));
					cx.stop_propagation();
				}),
			)
			.on_click(cx.listener(|s, event: &gpui::ClickEvent, _, cx| {
				if event.click_count() == 2 {
					s.sidebar_width = 192.0;
					cx.notify();
				}
			}))
			.on_key_down(cx.listener(|s, event: &gpui::KeyDownEvent, window, cx| {
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

	pub(super) fn workspace_resize_root(
		&self,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		div()
			.id("chief-workspace")
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

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn sidebar_drag_tracks_pointer_and_stops_on_release(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(gpui::size(px(1400.0), px(900.0)));
		surface.update(visual, |s, cx| s.visual_workspace_fixture(cx));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let point = |x| gpui::point(px(x), px(300.0));
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

	#[test]
	fn graph_content_caps_and_expansion_are_independent() {
		let mut layout = graph::Layout::default();
		layout.nodes.push(graph::Node { id: "one".into(), x: 20.0, y: 32.0 });
		let small = graph_size(&layout, 0.85, (1100.0, 800.0), false);
		assert_eq!(small, (1100.0, 180.0));
		layout.nodes.push(graph::Node { id: "far".into(), x: 1400.0, y: 1600.0 });
		assert_eq!(graph_size(&layout, 0.85, (1100.0, 800.0), false), (1100.0, 360.0));
		assert_eq!(graph_size(&layout, 0.85, (700.0, 300.0), false), (700.0, 60.0));
		assert_eq!(graph_size(&layout, 0.85, (1100.0, 800.0), true), (1100.0, 800.0));
	}
	#[test]
	fn sidebar_limits_preserve_main_space() {
		assert_eq!(sidebar_width(80.0, 1200.0), 160.0);
		assert_eq!(sidebar_width(500.0, 1200.0), 360.0);
		assert_eq!(sidebar_width(300.0, 800.0), 200.0);
		assert_eq!(sidebar_width(256.0, 1200.0), 256.0);
	}
}
