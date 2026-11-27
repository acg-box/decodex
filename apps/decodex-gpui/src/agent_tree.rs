//! Right-panel placement, sharing the existing workspace resize behavior.
use gpui::{
	AnyElement, InteractiveElement as _,
	prelude::{IntoElement as _, ParentElement as _, Styled as _},
};

use crate::{
	shell::agent_surface::{AgentSurface, Context, Window},
	ui_theme,
};

impl AgentSurface {
	pub(super) fn restore_collaboration_sidebar(&mut self, previous_page: Option<String>) {
		let current_page = self.conversation_page();
		if previous_page == current_page {
			return;
		}
		if let Some(previous) = previous_page {
			if self.workspace.agent_tree_visible {
				self.workspace.collaboration_open_pages.insert(previous);
			} else {
				self.workspace.collaboration_open_pages.remove(&previous);
			}
		}
		self.reset_reference();
		self.workspace.agent_tree_visible = current_page
			.is_some_and(|page| self.workspace.collaboration_open_pages.contains(&page));
	}

	pub(crate) fn toggle_agent_tree(&mut self, cx: &mut Context<Self>) {
		if self.workspace_connecting() {
			return;
		}

		crate::ui_motion::begin_interaction();
		self.workspace.agent_tree_visible = !self.workspace.agent_tree_visible;

		cx.notify();
	}

	pub(super) fn agent_tree_width(&self, window: &Window) -> f32 {
		self.workspace_panel_layout(window).right_slot
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
