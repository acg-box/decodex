//! Right-panel placement, sharing the existing workspace resize behavior.
use gpui::{
	AnyElement, MouseDownEvent,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		Styled as _,
	},
};

use crate::{
	shell::agent_surface::{AgentSurface, Context, Window},
	ui_motion,
	ui_theme::{self, WORKSPACE_GUTTER},
};

impl AgentSurface {
	pub(crate) fn toggle_agent_tree(&mut self, cx: &mut Context<Self>) {
		if self.workspace_connecting() {
			return;
		}

		crate::ui_motion::begin_interaction();
		if !self.workspace.agent_tree_visible {
			self.mark_panel_opened(super::workspace_size::Panel::Right);
		}
		self.workspace.agent_tree_visible = !self.workspace.agent_tree_visible;
		if !self.workspace.agent_tree_visible && self.workspace.sidebar_visible {
			self.workspace.panel_priority = super::workspace_size::Panel::Left;
		}

		cx.notify();
	}

	pub(super) fn agent_tree_width(&self, window: &Window) -> f32 {
		self.workspace_panel_layout(window).right_slot
	}

	pub(super) fn activity_pane(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		let layout = self.workspace_panel_layout(window);
		let width = self.workspace_right_display_width(window);
		let floating =
			self.workspace_panel_floating(super::workspace_size::Panel::Right, layout, width);
		if width <= 0.1 {
			self.hide_workspace_material(super::workspace_size::Panel::Right, window, cx);
		}
		if self
			.workspace
			.right_content_motion
			.borrow()
			.as_ref()
			.is_some_and(|t| t.moving(std::time::Instant::now()))
		{
			ui_motion::request_frame(window, cx);
		}
		let content_width = self
			.workspace
			.agent_panel_width
			.clamp(160., 480.)
			.min((f32::from(window.viewport_size().width) - 2. * WORKSPACE_GUTTER).max(0.));
		self.deliveries.update(cx, |panel, cx| panel.set_width(content_width, cx));
		ui_theme::workspace_panel_frame()
			.id("workspace-activity-pane")
			.debug_selector(|| "workspace-activity-pane".into())
			.absolute()
			.right(gpui::px(WORKSPACE_GUTTER))
			.top(gpui::px(crate::shell::WINDOW_CONTROLS_CLEARANCE))
			.bottom(gpui::px(WORKSPACE_GUTTER))
			.w(gpui::px(width))
			.overflow_hidden()
			.opacity(self.workspace.arrival.progress(0.08))
			.when(width > 0.1, |panel| {
				panel.child(self.workspace_material(
					super::workspace_size::Panel::Right,
					width,
					content_width,
					window,
					cx,
				))
			})
			.when(floating, |panel| {
				panel.on_mouse_down_out(cx.listener(|s, event: &MouseDownEvent, window, cx| {
					if event.position.y >= gpui::px(crate::shell::WINDOW_CONTROLS_CLEARANCE)
						&& f32::from(event.position.x)
							> s.workspace_sidebar_width(window) + WORKSPACE_GUTTER
					{
						s.dismiss_workspace_overlays(window, cx);
					}
				}))
			})
			.into_any_element()
	}

	pub(super) fn agent_tree(&self, cx: &mut Context<Self>) -> AnyElement {
		ui_theme::workspace_panel_content()
			.id("agent-panel-focus")
			.relative()
			.size_full()
			.flex()
			.flex_col()
			.child(self.render_agent_workbench(cx))
			.into_any_element()
	}
}
