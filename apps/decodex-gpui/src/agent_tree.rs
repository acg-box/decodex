//! Right-panel placement, sharing the existing workspace resize behavior.
use gpui::{
	AnyElement,
	prelude::{InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _},
};

use crate::{
	shell::agent_surface::{AgentSurface, Context, Window, workspace_size::Panel},
	ui_theme::WORKSPACE_GUTTER,
};

impl AgentSurface {
	pub(crate) fn toggle_agent_tree(&mut self, cx: &mut Context<Self>) {
		if self.workspace_connecting() {
			return;
		}

		self.workspace.agent_tree_visible = !self.workspace.agent_tree_visible;

		cx.notify();
	}

	pub(super) fn agent_tree_width(&self, window: &Window) -> f32 {
		if !self.workspace.agent_tree_visible
			|| self.workspace.chat_expanded
			|| !self.reserve_workspace_panels()
		{
			return 0.0;
		}

		let width = f32::from(window.viewport_size().width);
		let left = self.workspace_sidebar_width(window);

		self.workspace
			.agent_panel_width
			.min((width - left - 440.0 - 3. * WORKSPACE_GUTTER).max(0.0))
	}

	pub(super) fn agent_tree(&self, cx: &mut Context<Self>) -> AnyElement {
		gpui::div()
			.id("agent-panel-focus")
			.relative()
			.size_full()
			.flex()
			.flex_col()
			.capture_any_mouse_down(
				cx.listener(|s, _, _, _| s.workspace.focused_panel = Some(Panel::Right)),
			)
			.child(self.render_agent_workbench(cx))
			.into_any_element()
	}
}
