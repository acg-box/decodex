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
		(
			self.workspace.sidebar_width,
			self.workspace.agent_panel_width,
			self.workspace.graph_panel_height,
		)
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
			self.workspace.sidebar_visible && width > 1_000.0,
			self.workspace.agent_tree_visible && self.has_work() && !self.workspace.graph_expanded,
			self.workspace.graph_visible
				&& !self.workspace.dock_compact
				&& self.reserve_workspace_panels()
				&& !self.workspace.graph_expanded,
		];

		for (index, panel) in [Panel::Left, Panel::Right, Panel::Bottom].into_iter().enumerate() {
			if !visible[index] || (!all && self.workspace.focused_panel != Some(panel)) {
				continue;
			}

			if panel == Panel::Bottom {
				self.workspace.dock_compact = false;
			}
			let (value, default, min, max) = match panel {
				Panel::Left => (&mut self.workspace.sidebar_width, defaults.sidebar, 160.0, 480.0),
				Panel::Right =>
					(&mut self.workspace.agent_panel_width, defaults.sidebar, 160.0, 480.0),
				Panel::Bottom =>
					(&mut self.workspace.graph_panel_height, defaults.dock, 120.0, 640.0),
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
			.fold((0.0_f32, 0.0_f32), |(x, y), node| (x.max(node.x + 190.0), y.max(node.y + 66.0)));
		self.workspace.graph_display_zoom *= ((width - 20.0) / (right + 20.0))
			.min((height - 48.).max(32.) / (bottom + 12.))
			.clamp(0.1, 1.0);
		let zoom = self.workspace.graph_display_zoom;

		self.workspace.graph_inset = (
			((width - (right + 20.0) * zoom) / 2.0).max(0.0),
			if self.workspace.graph_expanded {
				((height - 118.0 - (bottom + 32.0) * zoom) / 2.0).max(0.0)
			} else {
				0.0
			},
		);
	}

	pub(super) fn workspace_graph_layout(&self) -> graph::Layout {
		let mut layout = self.workspace_graph_full_layout();
		if !self.workspace.dock_completed {
			self.collapse_graph_completed(&mut layout);
		}
		layout
	}

	pub(super) fn collapse_graph_completed(&self, layout: &mut graph::Layout) -> usize {
		let Some(snapshot) = &self.snapshot else { return 0 };
		layout.retain_prerequisites(|id| {
			snapshot
				.work_items
				.iter()
				.any(|w| w.id == id && super::dock::progress_state(snapshot, w).group != 3)
		})
	}

	pub(super) fn workspace_graph_full_layout(&self) -> graph::Layout {
		let Some(snapshot) = &self.snapshot else {
			return graph::Layout::default();
		};
		let scope = self.workspace.dock_record.clone().or_else(|| self.dock_scope());
		let mut layout = graph::Layout::new(snapshot, scope.as_deref());

		if layout.nodes.is_empty()
			&& let Some(work) = snapshot.work_items.iter().find(|w| Some(&w.id) == scope.as_ref())
		{
			if snapshot
				.dependencies
				.iter()
				.any(|e| e.work_item_id == work.id || e.depends_on_id == work.id)
			{
				layout = graph::Layout::new(snapshot, work.parent_goal_id.as_deref());
			}
			if layout.nodes.is_empty() {
				layout.nodes.push(graph::Node { id: work.id.clone(), x: 20., y: 32. });
			}
		}

		for node in &mut layout.nodes {
			let (x, y) = (node.x, node.y);

			node.x = 32.0 + (y - 32.0) / 112.0 * 224.0;
			node.y = 20.0 + (x - 20.0) / 188.0 * 96.0;
		}

		layout
	}

	fn sidebar_target_width(&self, window: &Window) -> f32 {
		let width = f32::from(window.viewport_size().width);
		if (self.workspace.sidebar_visible || self.workspace.sidebar_peek) && width > 1000. {
			sidebar_width(self.workspace.sidebar_width, width)
		} else {
			crate::ui_theme::CONVERSATION_TAB_SIZE + 2. * crate::ui_theme::SIDEBAR_INSET
		}
	}

	pub(super) fn workspace_sidebar_width(&self, window: &Window) -> f32 {
		let target = self.sidebar_target_width(window);
		let now = std::time::Instant::now();
		let mut motion = self.workspace.sidebar_motion.borrow_mut();
		let tween = motion.get_or_insert_with(|| ui_motion::Tween::new(target));
		if self.workspace.sidebar_drag.is_some() {
			*tween = ui_motion::Tween::new(target);
		} else {
			tween.target(target, now);
		}
		tween.sample(now)
	}

	pub(crate) fn topbar_insets(&self, window: &Window) -> (f32, f32) {
		// Match the actual conversation column, including animated sidebar widths.
		(self.workspace_sidebar_width(window), self.agent_tree_width(window))
	}

	pub(super) fn workspace_graph_size(&self, window: &Window, _wide: bool) -> (f32, f32) {
		if !self.workspace.graph_visible || !self.reserve_workspace_panels() {
			return (0.0, 0.0);
		}

		let viewport = window.viewport_size();
		let sidebar = self.workspace_sidebar_width(window);
		let available = (
			f32::from(viewport.width) - sidebar - self.agent_tree_width(window),
			(f32::from(viewport.height) - WINDOW_CONTROLS_CLEARANCE).max(0.0),
		);

		if self.workspace.graph_expanded {
			return available;
		}
		let height = if self.workspace.dock_compact {
			38.
		} else {
			self.workspace.graph_panel_height.clamp(120., 640.).min((available.1 - 240.).max(38.))
		};
		(available.0, height.min(available.1))
	}

	pub(super) fn sidebar_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
		self.workspace.sidebar_leave = None;
		if self.workspace.sidebar_visible {
			return;
		}
		if hovered {
			self.workspace.sidebar_peek = true;
			cx.notify();
		} else {
			self.workspace.sidebar_leave = Some(cx.spawn(async |surface, cx| {
				cx.background_executor().timer(std::time::Duration::from_millis(220)).await;
				let _ = surface.update(cx, |s, cx| {
					s.workspace.sidebar_peek = false;
					cx.notify();
				});
			}));
		}
	}

	pub(super) fn sidebar_slot(
		&self,
		_wide: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let width = self.workspace_sidebar_width(window);
		if self
			.workspace
			.sidebar_motion
			.borrow()
			.as_ref()
			.is_some_and(|tween| tween.moving(std::time::Instant::now()))
		{
			ui_motion::request_frame(window, cx);
		}
		gpui::div()
			.id("left-panel-slot")
			.debug_selector(|| "left-panel-slot".into())
			.on_hover(cx.listener(|s, hovered: &bool, _, cx| s.sidebar_hover(*hovered, cx)))
			.flex_none()
			.w(gpui::px(width))
			.h_full()
			.overflow_hidden()
			.capture_any_mouse_down(
				cx.listener(|s, _, _, _| s.workspace.focused_panel = Some(Panel::Left)),
			)
			.child(self.workspace_sidebar(cx))
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
					s.workspace.sidebar_drag = Some((
						event.position.x.into(),
						sidebar_width(
							s.workspace.sidebar_width,
							window.viewport_size().width.into(),
						),
					));

					cx.stop_propagation();
				}),
			)
			.on_click(cx.listener(|s, event: &ClickEvent, _, cx| {
				if event.click_count() == 2 {
					s.workspace.sidebar_width = PanelDefaults::configured().sidebar.into();

					cx.notify();
				}
			}))
			.on_key_down(cx.listener(|s, event: &KeyDownEvent, window, cx| {
				let delta = match event.keystroke.key.as_str() {
					"left" => -16.0,
					"right" => 16.0,
					_ => return,
				};

				s.workspace.sidebar_width = sidebar_width(
					s.workspace.sidebar_width + delta,
					window.viewport_size().width.into(),
				);

				cx.stop_propagation();
				cx.notify();
			}))
	}

	pub(super) fn workspace_resize_root(&self, cx: &mut Context<Self>) -> Stateful<Div> {
		gpui::div()
			.id("agent-workspace")
			.relative()
			.on_mouse_move(cx.listener(|s, event: &MouseMoveEvent, window, cx| {
				let Some((start, width)) = s.workspace.sidebar_drag else {
					return;
				};

				if event.pressed_button != Some(MouseButton::Left) {
					s.workspace.sidebar_drag = None;

					return;
				}

				s.workspace.sidebar_width = sidebar_width(
					width + f32::from(event.position.x) - start,
					window.viewport_size().width.into(),
				);

				cx.stop_propagation();
				cx.notify();
			}))
			.on_mouse_up(
				MouseButton::Left,
				cx.listener(|s, _, _, _| {
					s.workspace.sidebar_drag = None;
				}),
			)
			.on_mouse_up_out(
				MouseButton::Left,
				cx.listener(|s, _, _, _| {
					s.workspace.sidebar_drag = None;
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
	fn sidebar_hover_uses_pinned_layout_and_reentry_cancels_close(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.sidebar_visible = false;
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear());
		std::thread::sleep(std::time::Duration::from_millis(250));
		visual.update(|w, cx| w.draw(cx).clear());
		let collapsed = visual.debug_bounds("workspace-transcript").unwrap();
		let mark = "conversation-mark-agent";
		let collapsed_mark = visual.debug_bounds(mark).unwrap();
		let row = visual.debug_bounds("page-agent").unwrap();
		assert_eq!(row.size.width, row.size.height, "collapsed conversation targets are square");
		surface.update(visual, |s, cx| s.sidebar_hover(true, cx));
		visual.update(|w, cx| w.draw(cx).clear());
		std::thread::sleep(std::time::Duration::from_millis(250));
		visual.update(|w, cx| w.draw(cx).clear());
		let expanded = visual.debug_bounds("workspace-transcript").unwrap();
		assert!(expanded.origin.x > collapsed.origin.x);
		assert_eq!(visual.debug_bounds(mark).unwrap(), collapsed_mark);
		surface.update(visual, |s, cx| {
			s.workspace.sidebar_visible = true;
			s.workspace.sidebar_peek = false;
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear());
		assert_eq!(visual.debug_bounds("workspace-transcript").unwrap(), expanded);
		surface.update(visual, |s, _| s.workspace.sidebar_visible = false);
		surface.update(visual, |s, cx| {
			s.sidebar_hover(false, cx);
			s.sidebar_hover(true, cx);
		});
		visual.run_until_parked();
		visual.executor().advance_clock(std::time::Duration::from_millis(250));
		visual.run_until_parked();
		surface.read_with(visual, |s, _| assert!(s.workspace.sidebar_peek));
		surface.update(visual, |s, cx| s.sidebar_hover(false, cx));
		visual.run_until_parked();
		visual.executor().advance_clock(std::time::Duration::from_millis(250));
		visual.run_until_parked();
		surface.read_with(visual, |s, _| assert!(!s.workspace.sidebar_peek));
	}

	#[gpui::test]
	fn sidebar_drag_tracks_pointer_and_stops_on_release(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.0), gpui::px(900.0)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.sidebar_width = 192.0;
			// Start the drag fixture at its settled width.
			s.workspace.sidebar_motion = Default::default();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let point = |x| gpui::point(gpui::px(x), gpui::px(300.0));

		visual.simulate_mouse_down(point(189.0), MouseButton::Left, Default::default());
		visual.simulate_mouse_move(point(269.0), MouseButton::Left, Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.workspace.sidebar_width, 272.0);
			assert_eq!(s.workspace.graph_pan, (0.0, 0.0));
		});
		visual.simulate_mouse_up(point(269.0), MouseButton::Left, Default::default());
		visual.simulate_mouse_move(point(400.0), None, Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.workspace.sidebar_width, 272.0);
			assert!(s.workspace.sidebar_drag.is_none());
		});
	}

	#[gpui::test]
	fn panel_shortcuts_resize_only_the_selected_visible_panels(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.0), gpui::px(900.0)));

		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.workspace.sidebar_width = 240.0;
				s.workspace.agent_panel_width = 240.0;
				s.workspace.graph_panel_height = 240.0;
				s.workspace.agent_tree_visible = true;
				s.workspace.graph_visible = true;
				s.workspace.graph_expanded = false;
				s.workspace.focused_panel = Some(Panel::Right);
				s.workspace.dock_record = Some("release".into());

				s.resize_panel(24.0, false, false, window, cx);

				assert_eq!(
					(
						s.workspace.sidebar_width,
						s.workspace.agent_panel_width,
						s.workspace.graph_panel_height
					),
					(240.0, 264.0, 240.0)
				);

				s.workspace.sidebar_visible = false;

				s.resize_panel(-24.0, false, true, window, cx);

				assert_eq!(
					(
						s.workspace.sidebar_width,
						s.workspace.agent_panel_width,
						s.workspace.graph_panel_height
					),
					(240.0, 240.0, 216.0)
				);

				s.resize_panel(0.0, true, true, window, cx);

				assert_eq!(
					s.workspace.graph_panel_height,
					f32::from(crate::panel_preferences::PanelDefaults::configured().dock)
				);

				s.workspace.focused_panel = None;

				s.resize_panel(24.0, false, false, window, cx);

				assert_eq!(
					s.workspace.agent_panel_width,
					f32::from(crate::panel_preferences::PanelDefaults::configured().sidebar)
				);
			})
		});
	}

	#[gpui::test]
	fn graph_expands_restores_and_preserves_conversation_space(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_200.), gpui::px(900.)));

		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.workspace.sidebar_visible = false;
				s.workspace.agent_tree_visible = false;
				s.workspace.graph_visible = true;
				s.workspace.graph_expanded = false;
				s.workspace.graph_panel_height = 275.;
				// Geometry assertions below describe the settled panel layout.
				s.workspace.sidebar_motion = Default::default();

				assert_eq!(s.workspace_graph_size(window, true).1, 275.);

				s.workspace.graph_zoom = 1.8;
				s.workspace.graph_pan = (800., 600.);

				assert_eq!(s.workspace_graph_size(window, true).1, 275.);

				s.workspace.graph_expanded = true;

				assert_eq!(
					s.workspace_graph_size(window, true).1,
					900. - super::WINDOW_CONTROLS_CLEARANCE
				);

				s.workspace.graph_visible = false;

				assert_eq!(s.workspace_graph_size(window, true), (0., 0.));

				s.workspace.graph_visible = true;
				s.workspace.graph_expanded = false;
			});
		});

		visual.simulate_resize(gpui::size(gpui::px(1_200.), gpui::px(300.)));

		visual.update(|window, cx| {
			surface.update(cx, |s, _| {
				let (_, height) = s.workspace_graph_size(window, true);

				assert_eq!(height, 38.);
				assert_eq!(
					s.workspace.graph_panel_height, 275.,
					"small windows retain the requested height"
				);
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
