//! Content bounds and direct manipulation for the Agent workspace.

use std::time::Instant;

use gpui::{
	AnyElement, ClickEvent, Div, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
	Stateful, prelude::FluentBuilder as _,
};

use crate::{
	panel_preferences::PanelDefaults,
	shell::{
		WINDOW_CONTROLS_CLEARANCE,
		agent_surface::{
			AgentSurface, Context, InteractiveElement, IntoElement, ParentElement, Role,
			StatefulInteractiveElement, Styled, Window, dock,
			graph::{self, Node},
		},
	},
	ui_layout,
	ui_motion::{self, Tween},
	ui_theme::{
		self, ACCENT, CONVERSATION_HEADER_HEIGHT, CONVERSATION_TAB_SIZE, PANEL_DIVIDER,
		SIDEBAR_INSET, TEXT_MUTED, WORKSPACE_GUTTER,
	},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Panel {
	Left,
	Right,
	Bottom,
}

pub(super) const MAIN_MIN_WIDTH: f32 = 360.;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PanelLayout {
	pub left_slot: f32,
	pub right_slot: f32,
	pub left_width: f32,
	pub right_width: f32,
	pub left_floating: bool,
	pub right_floating: bool,
}

impl PanelLayout {
	fn allocate(
		width: f32,
		left: Option<f32>,
		right: Option<f32>,
		first: Panel,
		active: Panel,
	) -> Self {
		let rail = CONVERSATION_TAB_SIZE + 2. * SIDEBAR_INSET;
		let mut layout = Self { left_slot: rail, left_width: rail, ..Self::default() };
		let mut room = width - 2. * WORKSPACE_GUTTER - rail - WORKSPACE_GUTTER - MAIN_MIN_WIDTH;
		let order = if first == Panel::Right {
			[Panel::Right, Panel::Left]
		} else {
			[Panel::Left, Panel::Right]
		};
		for panel in order {
			let requested = if panel == Panel::Left { left } else { right };
			let Some(requested) = requested else { continue };
			let cost =
				if panel == Panel::Left { requested - rail } else { requested + WORKSPACE_GUTTER };
			if room >= cost {
				room -= cost;
				if panel == Panel::Left {
					layout.left_slot = requested;
				} else {
					layout.right_slot = requested;
				}
			}
			if panel == Panel::Left {
				layout.left_width = requested;
				layout.left_floating = layout.left_slot < requested;
			} else {
				layout.right_width = requested;
				layout.right_floating = layout.right_slot == 0.;
			}
		}
		// A very narrow window has room for one temporary surface at a time.
		if layout.left_floating && layout.right_floating {
			if active == Panel::Left {
				layout.right_width = 0.;
				layout.right_floating = false;
			} else {
				layout.left_width = rail;
				layout.left_floating = false;
			}
		}
		layout
	}
}

impl AgentSurface {
	pub(super) fn workspace_panel_layout(&self, window: &Window) -> PanelLayout {
		if self.workspace.chat_expanded {
			return PanelLayout::default();
		}
		let width = f32::from(window.viewport_size().width);
		let mut layout = PanelLayout::allocate(
			width,
			self.workspace.sidebar_visible.then(|| self.workspace_sidebar_expanded_width(window)),
			(self.workspace.agent_tree_visible && self.reserve_workspace_panels())
				.then(|| sidebar_width(self.workspace.agent_panel_width, width)),
			self.workspace.panel_priority,
			self.workspace.last_opened_panel,
		);
		if self.workspace.sidebar_peek && !self.workspace.sidebar_visible && !layout.right_floating
		{
			layout.left_width = self.workspace_sidebar_expanded_width(window);
			layout.left_floating = true;
		}
		layout
	}

	pub(super) fn mark_panel_opened(&mut self, panel: Panel) {
		if (panel == Panel::Left && !self.workspace.agent_tree_visible)
			|| (panel == Panel::Right && !self.workspace.sidebar_visible)
		{
			self.workspace.panel_priority = panel;
		}
		self.workspace.last_opened_panel = panel;
	}

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
		let layout = self.workspace_panel_layout(window);
		let visible = [
			self.workspace.sidebar_visible && layout.left_width > 0.,
			layout.right_width > 0.,
			self.workspace.graph_visible
				&& self.reserve_workspace_panels()
				&& !self.workspace.graph_expanded,
		];

		for (index, panel) in [Panel::Left, Panel::Right, Panel::Bottom].into_iter().enumerate() {
			if !visible[index] || (!all && self.workspace.focused_panel != Some(panel)) {
				continue;
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

	pub(super) fn graph_node_size(&self, id: &str) -> (f32, f32) {
		if self.handoffs.focus.as_deref() == Some(id) { (360., 180.) } else { (190., 66.) }
	}

	pub(super) fn update_graph_inset(&mut self, width: f32, height: f32) {
		let layout = self.workspace_graph_layout();
		let (right, bottom) = layout.nodes.iter().fold((0.0_f32, 0.0_f32), |(x, y), node| {
			let (width, height) = self.graph_node_size(&node.id);

			(x.max(node.x + width), y.max(node.y + height))
		});

		self.workspace.graph_fit_zoom =
			(width / (right + 20.)).min((height - 38.).max(1.) / (bottom + 20.)).clamp(0.35, 1.);

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
				.any(|w| w.id == id && dock::progress_state(snapshot, w).group != 3)
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
				layout.nodes.push(Node { id: work.id.clone(), x: 20., y: 32. });
			}
		}
		// Dependency chains already explain the flow. Keep the owner in the heading,
		// rather than drawing a second set of ownership lines across those chains.
		if !layout.edges.is_empty() && !layout.reports.is_empty() {
			layout.nodes.pop();
			layout.reports.clear();

			for node in &mut layout.nodes {
				node.y -= 112.;
			}
		}

		for node in &mut layout.nodes {
			let (x, y) = (node.x, node.y);

			node.x = 32.0 + (y - 32.0) / 112.0 * 224.0;
			node.y = 20.0 + (x - 20.0) / 188.0 * 96.0;
		}

		if let Some(focus) = self.handoffs.focus.as_ref()
			&& let Some(column) =
				layout.nodes.iter().find(|node| &node.id == focus).map(|node| node.x)
		{
			let mut row = 230.;

			for node in &mut layout.nodes {
				if &node.id == focus {
					node.y = 20.;
				} else if node.x == column {
					node.y = row;
					row += 96.;
				} else if node.x > column {
					node.x += 170.;
				}
			}
		}

		layout
	}

	fn sidebar_target_width(&self, window: &Window) -> f32 {
		self.workspace_panel_layout(window).left_slot
	}

	pub(super) fn workspace_sidebar_expanded(&self, window: &Window) -> bool {
		self.workspace_panel_layout(window).left_width > CONVERSATION_TAB_SIZE + 2. * SIDEBAR_INSET
	}

	pub(super) fn workspace_sidebar_expanded_width(&self, window: &Window) -> f32 {
		sidebar_width(self.workspace.sidebar_width, f32::from(window.viewport_size().width))
	}

	pub(super) fn workspace_sidebar_width(&self, window: &Window) -> f32 {
		let target = self.sidebar_target_width(window);
		let now = Instant::now();
		let mut motion = self.workspace.sidebar_motion.borrow_mut();
		let tween = motion.get_or_insert_with(|| Tween::new(target));

		if self.workspace.panel_drag.is_some_and(|(panel, _, _)| panel == Panel::Left) {
			*tween = Tween::new(target);
		} else {
			tween.target(target, now);
		}

		tween.sample(now)
	}

	pub(super) fn workspace_sidebar_display_width(&self, window: &Window) -> f32 {
		self.panel_motion_width(
			&self.workspace.sidebar_content_motion,
			self.workspace_panel_layout(window).left_width,
			Panel::Left,
		)
	}

	pub(super) fn workspace_right_display_width(&self, window: &Window) -> f32 {
		self.panel_motion_width(
			&self.workspace.right_content_motion,
			self.workspace_panel_layout(window).right_width,
			Panel::Right,
		)
	}

	fn panel_motion_width(
		&self,
		motion: &std::cell::RefCell<Option<Tween>>,
		target: f32,
		panel: Panel,
	) -> f32 {
		let now = Instant::now();
		let mut motion = motion.borrow_mut();
		let tween = motion.get_or_insert_with(|| Tween::new(target));
		if self.workspace.panel_drag.is_some_and(|(dragged, _, _)| dragged == panel) {
			*tween = Tween::new(target);
		} else {
			tween.target(target, now);
		}
		tween.sample(now)
	}

	pub(super) fn workspace_composer_clearance(&self) -> f32 {
		self.workspace.composer_overlay_height.max(0.)
	}

	/// Keep the native child window behind temporary workspace panels without
	/// changing the editor's layout or its input state.
	pub(super) fn workspace_composer_clip(
		&self,
		bounds: gpui::Bounds<gpui::Pixels>,
		window: &Window,
	) -> gpui::Bounds<gpui::Pixels> {
		let layout = self.workspace_panel_layout(window);
		let width = f32::from(bounds.size.width);
		let left_width = self.workspace_sidebar_display_width(window);
		let right_width = self.workspace_right_display_width(window);
		let left = if left_width > layout.left_slot + 0.5 {
			(WORKSPACE_GUTTER + left_width - f32::from(bounds.left())).clamp(0., width)
		} else {
			0.
		};
		let right = if right_width > layout.right_slot + 0.5 {
			(f32::from(window.viewport_size().width)
				- WORKSPACE_GUTTER
				- right_width
				- f32::from(bounds.left()))
			.clamp(0., width)
		} else {
			width
		};
		gpui::Bounds::new(
			gpui::point(gpui::px(left), gpui::px(0.)),
			gpui::size(gpui::px((right - left).max(0.)), bounds.size.height),
		)
	}

	pub(super) fn workspace_graph_size(&self, window: &Window, _wide: bool) -> (f32, f32) {
		if !self.workspace.graph_visible
			|| self.workspace.chat_expanded
			|| !self.reserve_workspace_panels()
		{
			return (0.0, 0.0);
		}

		let viewport = window.viewport_size();
		let sidebar = self.workspace_sidebar_width(window);
		let right = self.agent_tree_width(window);
		let gutters =
			2. * WORKSPACE_GUTTER + sidebar.min(WORKSPACE_GUTTER) + right.min(WORKSPACE_GUTTER);
		let available = (
			(f32::from(viewport.width) - sidebar - right - gutters).max(0.),
			(f32::from(viewport.height) - WINDOW_CONTROLS_CLEARANCE - WORKSPACE_GUTTER).max(0.0),
		);

		if self.workspace.graph_expanded {
			return available;
		}

		// Question choices can make the composer taller than a normal draft.
		// Retain a readable transcript and usable timeline above that overlay.
		let chat_minimum =
			(self.workspace_composer_clearance() + CONVERSATION_HEADER_HEIGHT + 160.).max(240.);
		let height = self
			.workspace
			.graph_panel_height
			.clamp(120., 640.)
			.min((available.1 - chat_minimum).max(38.));

		(available.0, height.min(available.1))
	}

	pub(super) fn sidebar_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
		if self.workspace.chat_expanded || self.workspace.sidebar_visible {
			self.workspace.sidebar_hover_intent.cancel();
			return;
		}
		self.workspace.sidebar_hover_intent.update(
			(),
			hovered,
			self.workspace.sidebar_peek.then_some(()),
			cx,
			|s, next, cx| {
				s.workspace.sidebar_hover_intent.settled();
				s.workspace.sidebar_peek = next.is_some();
				cx.notify();
			},
		);
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
			.is_some_and(|tween| tween.moving(Instant::now()))
		{
			ui_motion::request_frame(window, cx);
		}

		gpui::div()
			.id("left-panel-slot")
			.debug_selector(|| "left-panel-slot".into())
			.flex_none()
			.w(gpui::px(width))
			.mr(gpui::px(width.min(WORKSPACE_GUTTER)))
			.h_full()
			.into_any_element()
	}

	pub(super) fn sidebar_pane(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		let width = self.workspace_sidebar_display_width(window);
		let layout = self.workspace_panel_layout(window);
		let floating = layout.left_floating || width > self.workspace_sidebar_width(window) + 0.5;
		if !floating || width <= 0.1 {
			self.hide_workspace_material(Panel::Left, window, cx);
		}
		if self
			.workspace
			.sidebar_content_motion
			.borrow()
			.as_ref()
			.is_some_and(|t| t.moving(Instant::now()))
		{
			ui_motion::request_frame(window, cx);
		}
		ui_theme::workspace_surface()
			.id("workspace-navigation-pane")
			.debug_selector(|| "workspace-navigation-pane".into())
			.absolute()
			.left(gpui::px(WORKSPACE_GUTTER))
			.top(gpui::px(WINDOW_CONTROLS_CLEARANCE))
			.bottom(gpui::px(WORKSPACE_GUTTER))
			.w(gpui::px(width))
			.overflow_hidden()
			.opacity(self.workspace.arrival.progress(0.))
			.on_mouse_move(cx.listener(|s, event: &MouseMoveEvent, _, cx| {
				if event.pressed_button.is_none() && !cx.has_active_drag() {
					s.sidebar_hover(true, cx);
				}
			}))
			.on_hover(cx.listener(|s, hovered: &bool, _, cx| {
				if !hovered {
					s.sidebar_hover(false, cx);
				}
			}))
			.capture_any_mouse_down(
				cx.listener(|s, _, _, _| s.workspace.focused_panel = Some(Panel::Left)),
			)
			.when(width > 0.1, |panel| {
				if floating {
					panel.bg(gpui::transparent_black()).child(self.workspace_material(
						Panel::Left,
						width,
						self.workspace_sidebar_expanded_width(window),
						window,
						cx,
					))
				} else {
					panel.child(ui_layout::pane(self.presentations.sidebar.clone(), window))
				}
			})
			.when(width > 0.1 && self.workspace.sidebar_visible && !floating, |panel| {
				panel.child(self.sidebar_resize_handle(Panel::Left, cx))
			})
			.when(floating, |panel| {
				panel.on_mouse_down_out(cx.listener(|s, event: &MouseDownEvent, window, cx| {
					let right = s.agent_tree_width(window);
					if event.position.y >= gpui::px(WINDOW_CONTROLS_CLEARANCE)
						&& f32::from(event.position.x)
							< f32::from(window.viewport_size().width) - right - WORKSPACE_GUTTER
					{
						s.dismiss_workspace_overlays(window, cx);
					}
				}))
			})
			.into_any_element()
	}

	pub(crate) fn dismiss_workspace_overlays(
		&mut self,
		window: &Window,
		cx: &mut Context<Self>,
	) -> bool {
		if self.workspace.chat_expanded {
			return false;
		}
		let layout = self.workspace_panel_layout(window);
		let left = self.workspace.sidebar_peek
			|| (self.workspace.sidebar_visible
				&& layout.left_slot <= CONVERSATION_TAB_SIZE + 2. * SIDEBAR_INSET);
		let right = self.workspace.agent_tree_visible
			&& self.reserve_workspace_panels()
			&& layout.right_slot == 0.;
		if left {
			self.workspace.sidebar_visible = false;
			self.workspace.sidebar_peek = false;
			self.workspace.sidebar_hover_intent.cancel();
		}
		if right {
			self.workspace.agent_tree_visible = false;
		}
		if left || right {
			ui_motion::begin_interaction();
			cx.notify();
		}
		left || right
	}

	pub(super) fn sidebar_resize_handle(
		&self,
		panel: Panel,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let right = panel == Panel::Right;
		let group = if right { "activity-resize-handle" } else { "sidebar-resize-handle" };

		gpui::div()
			.id(if right { "agent-right-sidebar-resize" } else { "agent-sidebar-resize" })
			.debug_selector(move || {
				if right {
					"agent-right-sidebar-resize".into()
				} else {
					"agent-sidebar-resize".into()
				}
			})
			.absolute()
			.group(group)
			.when(right, |handle| handle.left_0())
			.when(!right, |handle| handle.right_0())
			.top_0()
			.bottom_0()
			.w(gpui::px(8.0))
			.flex()
			.items_center()
			.justify_center()
			.when(right, |handle| handle.border_l_1())
			.when(!right, |handle| handle.border_r_1())
			.border_color(gpui::transparent_black())
			.cursor_col_resize()
			.tab_index(0)
			.role(Role::Slider)
			.aria_label(if right {
				"Activity panel width. Drag or use Left and Right. Double-click to reset."
			} else {
				"Sidebar width. Drag or use Left and Right. Double-click to reset."
			})
			.hover(|s| s.border_color(ui_theme::rgb(ACCENT).opacity(0.65)))
			.focus_visible(|s| s.border_color(ui_theme::rgb(ACCENT)))
			.when(
				self.workspace.panel_drag.is_some_and(|(dragged, _, _)| dragged == panel),
				|handle| handle.border_color(ui_theme::rgb(ACCENT)),
			)
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(move |s, event: &MouseDownEvent, window, cx| {
					s.workspace.focused_panel = Some(panel);
					s.workspace.panel_drag = Some((
						panel,
						f32::from(event.position.x + window.bounds().origin.x),
						if right {
							s.workspace.agent_panel_width.clamp(160., 480.)
						} else {
							s.workspace.sidebar_width.clamp(160., 480.)
						},
					));

					cx.stop_propagation();
					cx.notify();
				}),
			)
			.on_click(cx.listener(move |s, event: &ClickEvent, _, cx| {
				if event.click_count() == 2 {
					if right {
						s.workspace.agent_panel_width = PanelDefaults::configured().sidebar.into();
					} else {
						s.workspace.sidebar_width = PanelDefaults::configured().sidebar.into();
					}

					cx.notify();
				}
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				let delta = match event.keystroke.key.as_str() {
					"left" => -16.0,
					"right" => 16.0,
					_ => return,
				};

				s.workspace.focused_panel = Some(panel);

				if right {
					s.workspace.agent_panel_width =
						(s.workspace.agent_panel_width - delta).clamp(160., 480.);
				} else {
					s.workspace.sidebar_width =
						(s.workspace.sidebar_width + delta).clamp(160., 480.);
				}

				cx.stop_propagation();
				cx.notify();
			}))
			.child(
				gpui::div()
					.w(gpui::px(2.))
					.h(gpui::px(28.))
					.rounded_full()
					.bg(ui_theme::rgba(PANEL_DIVIDER))
					.group_hover(group, |mark| mark.bg(ui_theme::rgb(ACCENT))),
			)
	}

	pub(super) fn dock_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.id("agent-dock-resize")
			.group("dock-resize-handle")
			.debug_selector(|| "agent-dock-resize".into())
			.absolute()
			.bottom_0()
			.left_0()
			.right_0()
			.h(gpui::px(8.))
			.bg(ui_theme::workspace_palette().conversation)
			.flex()
			.items_center()
			.justify_center()
			.border_b_1()
			.border_color(gpui::transparent_black())
			.cursor_row_resize()
			.tab_index(0)
			.role(Role::Slider)
			.aria_label("Dock height. Drag to resize. Double-click to reset. Control-Option minus or equals adjusts the selected panel.")
			.hover(|s| s.border_color(ui_theme::rgb(ACCENT).opacity(0.65)))
			.focus_visible(|s| s.border_color(ui_theme::rgb(ACCENT)))
			.when(self.workspace.panel_drag.is_some_and(|(panel, _, _)| panel == Panel::Bottom), |handle| {
				handle.border_color(ui_theme::rgb(ACCENT))
			})
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(|s, event: &MouseDownEvent, window, cx| {
					s.workspace.focused_panel = Some(Panel::Bottom);
					s.workspace.panel_drag = Some((
						Panel::Bottom,
						f32::from(event.position.y + window.bounds().origin.y),
						s.workspace_graph_size(window, true).1,
					));

					cx.stop_propagation();
					cx.notify();
				}),
			)
			.on_click(cx.listener(|s, event: &ClickEvent, window, cx| {
				if event.click_count() == 2 {
					s.resize_panel(0., true, false, window, cx);
				}
			}))
			.on_key_down(cx.listener(|s, event: &KeyDownEvent, window, cx| {
				let delta = match event.keystroke.key.as_str() {
					"up" => -16.,
					"down" => 16.,
					_ => return,
				};

				s.workspace.focused_panel = Some(Panel::Bottom);

				s.resize_panel(delta, false, false, window, cx);
				cx.stop_propagation();
			}))
			.child(gpui::div().w(gpui::px(32.)).h(gpui::px(2.)).rounded_full()
				.bg(ui_theme::rgb(TEXT_MUTED).opacity(0.3))
				.group_hover("dock-resize-handle", |mark| mark.bg(ui_theme::rgb(ACCENT))))
	}

	pub(super) fn workspace_resize_root(&self, cx: &mut Context<Self>) -> Stateful<Div> {
		gpui::div()
			.id("agent-workspace")
			.relative()
			.on_mouse_move(cx.listener(|s, event: &MouseMoveEvent, window, cx| {
				let Some((panel, start, width)) = s.workspace.panel_drag else {
					return;
				};

				if event.pressed_button != Some(MouseButton::Left) {
					s.workspace.panel_drag = None;

					cx.notify();

					return;
				}

				let delta = f32::from(event.position.x + window.bounds().origin.x) - start;

				if panel == Panel::Bottom {
					let delta = f32::from(event.position.y + window.bounds().origin.y) - start;

					s.workspace.graph_panel_height = (width + delta).clamp(120., 640.);
				} else if panel == Panel::Right {
					s.workspace.agent_panel_width = (width - delta).clamp(160., 480.);
				} else {
					s.workspace.sidebar_width = (width + delta).clamp(160., 480.);
				}

				cx.stop_propagation();
				cx.notify();
			}))
			.on_mouse_up(
				MouseButton::Left,
				cx.listener(|s, _, _, cx| {
					if s.workspace.panel_drag.take().is_some() {
						cx.notify();
					}
				}),
			)
			.on_mouse_up_out(
				MouseButton::Left,
				cx.listener(|s, _, _, cx| {
					if s.workspace.panel_drag.take().is_some() {
						cx.notify();
					}
				}),
			)
	}
}

fn sidebar_width(requested: f32, viewport: f32) -> f32 {
	requested.clamp(160.0, 480.0).min((viewport - 2. * WORKSPACE_GUTTER).max(0.))
}

#[cfg(test)]
mod tests {

	use std::thread;

	use crate::{
		shell::agent_surface::workspace_size::{self, AgentSurface, MouseButton, Panel},
		ui_theme::WORKSPACE_GUTTER,
	};

	#[gpui::test]
	fn focused_chat_hides_panels_while_graph_keeps_navigation(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.sidebar_visible = true;
			s.workspace.agent_tree_visible = true;
			s.workspace.graph_visible = true;
			s.workspace.sidebar_width = 230.;
			s.workspace.agent_panel_width = 270.;
			s.workspace.graph_panel_height = 300.;
		});

		for target in [None, Some(Panel::Bottom)] {
			surface.update(visual, |s, cx| {
				s.workspace.focused_panel = target;

				s.toggle_focused_content(cx);
			});
			visual.update(|window, cx| {
				let s = surface.read(cx);

				assert_eq!(
					s.sidebar_target_width(window),
					if target.is_none() { 0. } else { 230. }
				);
				assert_eq!(s.agent_tree_width(window), if target.is_none() { 0. } else { 270. });
				assert_eq!(s.workspace.chat_expanded, target.is_none());
				assert_eq!(s.workspace.graph_expanded, target.is_some());

				if target.is_none() {
					assert_eq!(s.workspace_graph_size(window, true), (0., 0.));
				}
			});

			visual.update(crate::ui_frame::draw_current_frame);

			thread::sleep(crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));

			visual.update(crate::ui_frame::draw_current_frame);

			let (restore, expand) = if target.is_none() {
				("chat-expansion-restore-icon", "chat-expansion-expand-icon")
			} else {
				("dock-expansion-restore-icon", "dock-expansion-expand-icon")
			};

			assert!(visual.debug_bounds(restore).is_some());
			assert!(visual.debug_bounds(expand).is_none());

			surface.update(visual, |s, cx| {
				s.toggle_focused_content(cx);

				assert!(!s.workspace.chat_expanded && !s.workspace.graph_expanded);
				assert!(
					s.workspace.sidebar_visible
						&& s.workspace.agent_tree_visible
						&& s.workspace.graph_visible
				);
				assert_eq!(
					(
						s.workspace.sidebar_width,
						s.workspace.agent_panel_width,
						s.workspace.graph_panel_height
					),
					(230., 270., 300.)
				);
			});
			visual.update(crate::ui_frame::draw_current_frame);

			thread::sleep(crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));

			visual.update(crate::ui_frame::draw_current_frame);

			assert!(visual.debug_bounds("chat-expansion-expand-icon").is_some());
			assert!(visual.debug_bounds("dock-expansion-expand-icon").is_some());
			assert!(visual.debug_bounds("chat-expansion-restore-icon").is_none());

			visual.update(|window, cx| {
				let s = surface.read(cx);

				assert_eq!(s.sidebar_target_width(window), 230.);
				assert_eq!(s.agent_tree_width(window), 270.);
			});

			assert!(visual.debug_bounds("dock-expansion-restore-icon").is_none());
		}
	}

	#[gpui::test]
	fn dock_resize_reserves_reading_space_above_a_tall_question(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_248.), gpui::px(840.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = true;
			s.workspace.graph_panel_height = 640.;
		});

		for overlay in [95., 235., 320.] {
			visual.update(|window, cx| {
				surface.update(cx, |s, _| {
					s.workspace.composer_overlay_height = overlay;

					let (_, dock_height) = s.workspace_graph_size(window, true);
					let reading_height = f32::from(window.viewport_size().height)
						- workspace_size::WINDOW_CONTROLS_CLEARANCE
						- dock_height
						- overlay;

					assert!(reading_height >= 160., "a question must not consume the timeline");
					assert_eq!(
						s.workspace.graph_panel_height, 640.,
						"retain the preferred Dock size"
					);
				})
			});
		}
	}

	#[gpui::test]
	fn top_execution_panel_reserves_space_above_chat(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.work_board.graph = false;
			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = false;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(250));

		visual.update(crate::ui_frame::draw_current_frame);

		let composer = visual.debug_bounds("floating-composer").unwrap();
		let transcript = visual.debug_bounds("workspace-transcript").unwrap();
		let header = visual.debug_bounds("work-dock").unwrap();
		let conversation_header = visual.debug_bounds("conversation-header").unwrap();
		let resize = visual.debug_bounds("agent-dock-resize").unwrap();
		let frame = visual.debug_bounds("workspace-center-frame").unwrap();
		let plane = visual.debug_bounds("workspace-content-plane").unwrap();
		let sidebar = visual.debug_bounds("left-panel-slot").unwrap();
		let activity = visual.debug_bounds("workspace-activity-frame").unwrap();

		assert_eq!(sidebar.left(), gpui::px(WORKSPACE_GUTTER));

		for side in [sidebar, activity] {
			assert_eq!(side.top(), frame.top(), "all columns start below the titlebar");
			assert_eq!(side.bottom(), frame.bottom(), "all columns share the bottom inset");
		}

		assert_eq!(frame.left() - sidebar.right(), gpui::px(WORKSPACE_GUTTER));
		assert_eq!(activity.left() - frame.right(), gpui::px(WORKSPACE_GUTTER));

		visual.update(|window, cx| {
			let (width, _) = surface.read(cx).workspace_graph_size(window, true);

			assert_eq!(frame.size.width, gpui::px(width), "the camera uses the inset working area");
		});

		assert_eq!(plane, frame);
		assert_eq!(composer.origin.x, frame.origin.x);
		assert_eq!(composer.size.width, frame.size.width);
		assert_eq!(composer.bottom(), frame.bottom());
		assert_eq!(header.size.width, plane.size.width);
		assert_eq!(resize.bottom(), header.bottom());
		assert_eq!(conversation_header.top(), header.bottom());
		assert!(transcript.top() >= conversation_header.bottom());
		assert!(visual.debug_bounds("dock-toggle").is_none());
		assert!(header.bottom() <= transcript.origin.y);

		for expanded in [true, false] {
			let toggle = visual.debug_bounds("graph-expand").unwrap();

			visual.simulate_click(toggle.center(), gpui::Modifiers::default());
			visual.update(crate::ui_frame::draw_current_frame);

			thread::sleep(std::time::Duration::from_millis(250));

			visual.update(crate::ui_frame::draw_current_frame);

			assert!(
				visual.debug_bounds("floating-composer").is_some(),
				"the central composer remains available while inspecting the map"
			);

			if !expanded {
				assert_eq!(visual.debug_bounds("floating-composer").unwrap(), composer);
				assert!(
					visual.debug_bounds("work-dock").unwrap().bottom()
						<= visual.debug_bounds("workspace-transcript").unwrap().origin.y
				);
			}
		}

		let close = visual.debug_bounds("graph-close").unwrap();

		visual.simulate_click(close.center(), gpui::Modifiers::default());
		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(250));

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("work-dock").is_none());

		surface.update(visual, |s, cx| s.toggle_workspace_graph(cx));
		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(250));

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("work-dock").unwrap().size.height > gpui::px(38.));
		assert!(visual.debug_bounds("dock-toggle").is_none());
	}

	#[gpui::test]
	fn unified_workspace_preserves_editor_across_pane_modes(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.work_board.graph = false;

			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
		});

		for (width, height) in [(1_400., 900.), (820., 620.)] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));

			for (graph, graph_full, chat_full, archived, browsing) in [
				(true, false, false, false, false),
				(false, false, false, false, false),
				(true, true, false, false, false),
				(true, false, true, false, false),
				(true, true, false, true, false),
				(true, true, false, false, true),
			] {
				surface.update(visual, |s, cx| {
					s.workspace.graph_visible = graph;
					s.workspace.graph_expanded = graph_full;
					s.workspace.chat_expanded = chat_full;
					s.workspace.browsing = browsing;

					let archived_ids = &mut s.snapshot.as_mut().unwrap().archived_work_ids;

					archived_ids.clear();

					if archived {
						archived_ids.insert("agent".into());
					}

					cx.notify();
				});

				visual.update(crate::ui_frame::draw_frame);

				thread::sleep(crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));

				visual.update(crate::ui_frame::draw_frame);

				let frame = visual.debug_bounds("workspace-center-frame").unwrap();
				let plane = visual.debug_bounds("workspace-content-plane").unwrap();

				assert_eq!(plane, frame);

				let composer = visual.debug_bounds("floating-composer");
				let has_composer = !browsing;

				assert_eq!(composer.is_some(), has_composer);

				if let Some(composer) = composer {
					assert_eq!(composer.origin.x, frame.origin.x);
					assert_eq!(composer.size.width, frame.size.width);
					assert_eq!(composer.bottom(), frame.bottom());
				}

				let backdrop = visual.debug_bounds("workspace-composer-backdrop");

				assert_eq!(backdrop.is_some(), graph_full && has_composer);

				if let Some(backdrop) = backdrop {
					assert_eq!(backdrop.origin.x, plane.origin.x);
					assert_eq!(backdrop.size.width, plane.size.width);
					assert_eq!(backdrop.bottom(), plane.bottom());
					assert_eq!(
						visual.debug_bounds("work-dock").unwrap().bottom(),
						backdrop.origin.y
					);
				}

				if graph_full && !has_composer {
					assert_eq!(visual.debug_bounds("work-dock").unwrap().bottom(), plane.bottom());
				}

				surface.update(visual, |s, cx| {
					assert_eq!(s.composer.read(cx).content(), "Keep this draft");
				});
			}
		}
	}

	#[gpui::test]
	fn sidebar_hover_expands_and_reentry_cancels_close_without_moving_the_rail_marks(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
		visual.simulate_mouse_move(
			gpui::point(gpui::px(1000.), gpui::px(800.)),
			None,
			Default::default(),
		);
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.sidebar_visible = false;
			s.workspace.sidebar_peek = false;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let collapsed = visual.debug_bounds("left-panel-slot").unwrap();
		let mark = visual.debug_bounds("conversation-mark-navigation-recent-agent").unwrap();
		let command = visual.debug_bounds("navigation-command-entry").unwrap().origin;
		assert_eq!(f32::from(collapsed.size.width), 40.);
		visual.simulate_mouse_move(collapsed.center(), None, Default::default());
		visual.run_until_parked();
		visual.executor().advance_clock(std::time::Duration::from_millis(90));
		visual.run_until_parked();
		surface.read_with(visual, |s, _| assert!(!s.workspace.sidebar_peek));
		let outside = gpui::point(gpui::px(1000.), gpui::px(800.));
		visual.simulate_mouse_move(outside, None, Default::default());
		visual.run_until_parked();
		visual.executor().advance_clock(crate::ui_motion::HOVER_OPEN_DELAY);
		visual.run_until_parked();
		surface.read_with(visual, |s, _| assert!(!s.workspace.sidebar_peek));
		visual.simulate_mouse_move(collapsed.center(), None, Default::default());
		visual.run_until_parked();
		visual.executor().advance_clock(crate::ui_motion::HOVER_OPEN_DELAY);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);
		thread::sleep(std::time::Duration::from_millis(250));
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("left-panel-slot").unwrap(), collapsed);
		assert!(
			visual.debug_bounds("workspace-navigation-pane").unwrap().size.width
				> collapsed.size.width
		);
		assert_eq!(visual.debug_bounds("conversation-mark-navigation-recent-agent").unwrap(), mark);
		assert_eq!(visual.debug_bounds("navigation-command-entry").unwrap().origin, command);
		surface.read_with(visual, |s, _| {
			assert!(s.workspace.sidebar_peek);
			assert!(!s.workspace.sidebar_visible, "hover must not pin the sidebar");
		});
		let projects = visual.debug_bounds("navigation-section-projects").unwrap();
		visual.simulate_click(projects.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert!(!s.workspace.sidebar_visible, "a section disclosure must not pin a peek");
		});
		visual.simulate_mouse_move(outside, None, Default::default());
		visual.executor().advance_clock(std::time::Duration::from_millis(100));
		visual.run_until_parked();
		visual.simulate_mouse_move(collapsed.center(), None, Default::default());
		visual.executor().advance_clock(std::time::Duration::from_millis(200));
		visual.run_until_parked();
		surface.read_with(visual, |s, _| assert!(s.workspace.sidebar_peek));
		visual.simulate_mouse_move(outside, None, Default::default());
		visual.executor().advance_clock(crate::ui_motion::HOVER_CLOSE_DELAY);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);
		thread::sleep(std::time::Duration::from_millis(250));
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("left-panel-slot").unwrap(), collapsed);
		surface.read_with(visual, |s, _| assert!(!s.workspace.sidebar_peek));
	}

	#[gpui::test]
	fn sidebar_motion_keeps_content_geometry_through_close_and_reversal(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.sidebar_visible = true;
			s.workspace.sidebar_motion = Default::default();
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let open = visual.debug_bounds("left-panel-slot").unwrap();
		let command = visual.debug_bounds("navigation-command-entry").unwrap();
		let mark = visual.debug_bounds("conversation-mark-navigation-recent-agent").unwrap();
		surface.update(visual, |s, cx| s.toggle_workspace_sidebar(cx));
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("navigation-command-entry").unwrap(), command);
		assert!(visual.debug_bounds("navigation-create-assistant").is_some());
		thread::sleep(std::time::Duration::from_millis(50));
		visual.update(crate::ui_frame::draw_frame);
		let closing = visual.debug_bounds("left-panel-slot").unwrap();
		assert!(closing.size.width < open.size.width && closing.size.width > gpui::px(40.));
		assert_eq!(visual.debug_bounds("navigation-command-entry").unwrap(), command);
		assert_eq!(visual.debug_bounds("conversation-mark-navigation-recent-agent").unwrap(), mark);
		surface.update(visual, |s, cx| s.toggle_workspace_sidebar(cx));
		visual.update(crate::ui_frame::draw_current_frame);
		let reversal = visual.debug_bounds("left-panel-slot").unwrap();
		assert!(reversal.size.width > gpui::px(40.) && reversal.size.width < open.size.width);
		thread::sleep(std::time::Duration::from_millis(50));
		visual.update(crate::ui_frame::draw_frame);
		let reopening = visual.debug_bounds("left-panel-slot").unwrap();
		assert!(
			reopening.size.width > reversal.size.width && reopening.size.width < open.size.width
		);
		assert_eq!(visual.debug_bounds("navigation-command-entry").unwrap(), command);
		thread::sleep(std::time::Duration::from_millis(220));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("left-panel-slot").unwrap(), open);
		surface.update(visual, |s, cx| s.toggle_workspace_sidebar(cx));
		visual.update(crate::ui_frame::draw_current_frame);
		thread::sleep(std::time::Duration::from_millis(220));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("left-panel-slot").unwrap().size.width, gpui::px(40.));
		assert_eq!(
			visual.debug_bounds("navigation-command-entry").unwrap().size.width,
			gpui::px(28.)
		);
		assert!(visual.debug_bounds("navigation-create-assistant").is_none());
		assert_eq!(visual.debug_bounds("conversation-mark-navigation-recent-agent").unwrap(), mark);
	}

	#[gpui::test]
	fn panel_drag_tracks_pointer_and_stops_on_release(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.0), gpui::px(900.0)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.sidebar_width = 192.0;
			// Start the drag fixture at its settled width.
			s.workspace.sidebar_motion = Default::default();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		let start =
			visual.debug_bounds("agent-sidebar-resize").expect("sidebar resize handle").center();
		let end = start + gpui::point(gpui::px(80.), gpui::px(0.));

		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.workspace.sidebar_width, 272.0);
			assert_eq!(s.workspace.graph_pan, (0.0, 0.0));
		});
		visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(
			end + gpui::point(gpui::px(120.), gpui::px(0.)),
			None,
			Default::default(),
		);
		surface.update(visual, |s, _| {
			assert_eq!(s.workspace.sidebar_width, 272.0);
			assert!(s.workspace.panel_drag.is_none());
		});
	}

	#[gpui::test]
	fn dock_drag_resizes_height_without_panning_and_stops_on_release(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = false;
			s.workspace.graph_panel_height = 300.;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		let start = visual.debug_bounds("agent-dock-resize").unwrap().center();
		let end = start + gpui::point(gpui::px(40.), gpui::px(80.));

		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
		visual.update(crate::ui_frame::draw_current_frame);

		assert_eq!(visual.debug_bounds("work-dock").unwrap().size.height, gpui::px(380.));

		surface.read_with(visual, |s, _| {
			assert_eq!(s.workspace.graph_panel_height, 380.);
			assert_eq!(s.workspace.focused_panel, Some(Panel::Bottom));
			assert_eq!(s.workspace.graph_pan, (0., 0.));
		});
		visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(start, None, Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.workspace.graph_panel_height, 380.);
			assert!(s.workspace.panel_drag.is_none());
		});

		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				s.resize_panel(-24., false, false, window, cx);

				assert_eq!(s.workspace.graph_panel_height, 356.);

				s.workspace.graph_expanded = true;

				cx.notify();
			})
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("agent-dock-resize").is_none());
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
					900. - super::WINDOW_CONTROLS_CLEARANCE - WORKSPACE_GUTTER
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

	#[gpui::test]
	fn right_sidebar_drag_grows_leftward_and_stops_on_release(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.agent_panel_width = 192.;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(250));

		visual.update(crate::ui_frame::draw_current_frame);

		let start = visual.debug_bounds("agent-right-sidebar-resize").unwrap().center();
		let end = start - gpui::point(gpui::px(80.), gpui::px(0.));

		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
		surface.read_with(visual, |s, _| assert_eq!(s.workspace.agent_panel_width, 272.));
		visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(start, None, Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.workspace.agent_panel_width, 272.);
			assert!(s.workspace.panel_drag.is_none());
		});
	}

	#[test]
	fn sidebar_limits_preserve_main_space() {
		assert_eq!(workspace_size::sidebar_width(80.0, 1_200.0), 160.0);
		assert_eq!(workspace_size::sidebar_width(500.0, 1_200.0), 480.0);
		assert_eq!(workspace_size::sidebar_width(300.0, 800.0), 300.0);
		assert_eq!(workspace_size::sidebar_width(256.0, 1_200.0), 256.0);
	}

	#[test]
	fn opening_the_other_panel_keeps_the_existing_slot() {
		let left_first = workspace_size::PanelLayout::allocate(
			800.,
			Some(240.),
			Some(240.),
			Panel::Left,
			Panel::Right,
		);
		assert_eq!((left_first.left_slot, left_first.right_slot), (240., 0.));
		assert!(left_first.right_floating);
		assert!(!left_first.left_floating);
		let right_first = workspace_size::PanelLayout::allocate(
			800.,
			Some(240.),
			Some(240.),
			Panel::Right,
			Panel::Left,
		);
		assert_eq!((right_first.left_slot, right_first.right_slot), (40., 240.));
		assert!(right_first.left_floating);
		assert!(!right_first.right_floating);
		let wide = workspace_size::PanelLayout::allocate(
			1200.,
			Some(240.),
			Some(240.),
			Panel::Right,
			Panel::Left,
		);
		assert_eq!((wide.left_slot, wide.right_slot), (240., 240.));
		assert!(!wide.left_floating && !wide.right_floating);
	}

	#[gpui::test]
	fn narrow_window_dismisses_temporary_panels_without_changing_graph(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(480.), gpui::px(360.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.graph_visible = true;
		});
		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				s.dismiss_workspace_overlays(window, cx);
				assert!(!s.workspace.sidebar_visible && !s.workspace.agent_tree_visible);
				assert!(s.workspace.graph_visible);
			})
		});
	}

	#[gpui::test]
	fn floating_panel_keeps_editor_graph_and_other_panel_geometry(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(800.), gpui::px(620.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.sidebar_width = 240.;
			s.workspace.agent_panel_width = 240.;
			s.workspace.agent_tree_visible = false;
			s.workspace.graph_visible = true;
			s.workspace.graph_zoom = 1.7;
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let before = visual.debug_bounds("workspace-center-frame").unwrap();
		surface.update(visual, |s, cx| s.toggle_agent_tree(cx));
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("workspace-center-frame").unwrap(), before);
		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				assert!(s.workspace_panel_layout(window).right_floating);
				s.workspace.right_content_motion = Default::default();
				let editor = gpui::Bounds::new(
					gpui::point(gpui::px(264.), gpui::px(500.)),
					gpui::size(gpui::px(512.), gpui::px(100.)),
				);
				let clip = s.workspace_composer_clip(editor, window);
				assert_eq!(clip.origin, gpui::point(gpui::px(0.), gpui::px(0.)));
				assert_eq!(clip.size.width, gpui::px(288.));
				assert_eq!(clip.size.height, editor.size.height);
				assert!(s.workspace.sidebar_visible && s.workspace.graph_visible);
				assert_eq!(s.workspace.graph_zoom, 1.7);
				assert_eq!(s.composer.read(cx).content(), "Keep this draft");
				s.dismiss_workspace_overlays(window, cx);
				assert!(s.workspace.sidebar_visible && !s.workspace.agent_tree_visible);
				s.workspace.right_content_motion = Default::default();
				assert_eq!(s.workspace_composer_clip(editor, window).size, editor.size);
			})
		});
	}
}
