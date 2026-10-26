//! Shared, borderless controls for expanding and closing workspace panes.
use gpui::{
	AnyElement, AppContext as _, KeyDownEvent,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	shell::{
		ControlTooltip,
		agent_surface::{AgentSurface, Context, Role, workspace},
		workspace_symbols::{self, Symbol},
	},
	ui_motion::SmoothControl as _,
	ui_theme::{self, BLUE, CHROME_CONTROL_SIZE, CONTROL_GROUP_HEIGHT, HOVER_FILL},
};

#[derive(Clone, Copy)]
pub(super) enum Panel {
	Conversation,
	Dock,
}

#[derive(Clone, Copy)]
enum Action {
	Toggle(Panel),
	CloseDock,
}

impl AgentSurface {
	pub(super) fn panel_controls(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
		let id = match panel {
			Panel::Conversation => "conversation-pane-controls",
			Panel::Dock => "relations-pane-controls",
		};
		let mut group = gpui::div()
			.id(id)
			.debug_selector(move || id.into())
			.h(gpui::px(CONTROL_GROUP_HEIGHT))
			.flex_none()
			.flex()
			.items_center()
			.gap_1()
			.child(self.panel_action(Action::Toggle(panel), cx));

		if matches!(panel, Panel::Dock) {
			group = group.child(self.panel_action(Action::CloseDock, cx));
		}

		group.into_any_element()
	}

	fn panel_action(&self, action: Action, cx: &mut Context<Self>) -> AnyElement {
		let (id, label, expanded, icon) = match action {
			Action::Toggle(panel) => {
				let (id, key, expanded, label) = match panel {
					Panel::Conversation => (
						"chat-expand",
						"chat-expansion",
						self.workspace.chat_expanded,
						"Expand conversation · Shift-Esc",
					),
					Panel::Dock => (
						"graph-expand",
						"dock-expansion",
						self.workspace.graph_expanded,
						"Expand Dock · Shift-Esc",
					),
				};

				(
					id,
					if expanded { "Restore layout · Shift-Esc" } else { label },
					Some(expanded),
					workspace_symbols::panel_expansion(key, expanded).into_any_element(),
				)
			},
			Action::CloseDock =>
				("graph-close", "Close Dock", None, workspace_symbols::icon(Symbol::Close)),
		};

		gpui::div()
			.id(id)
			.debug_selector(move || id.into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.when_some(expanded, |button, value| button.aria_expanded(value))
			.tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
			.size(gpui::px(CHROME_CONTROL_SIZE))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(5.))
			.border_1()
			.border_color(gpui::transparent_black())
			.cursor_pointer()
			.hover(|button| button.bg(ui_theme::rgba(HOVER_FILL)))
			.focus_visible(|button| button.border_color(ui_theme::rgb(BLUE)))
			.on_click(cx.listener(move |s, _, window, cx| {
				s.perform_panel_action(action, cx);

				workspace::restore_shell_focus(window, cx);

				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.perform_panel_action(action, cx);

					workspace::restore_shell_focus(window, cx);

					cx.stop_propagation();
				}
			}))
			.child(icon)
			.smooth()
			.into_any_element()
	}

	fn perform_panel_action(&mut self, action: Action, cx: &mut Context<Self>) {
		match action {
			Action::Toggle(Panel::Conversation) => self.toggle_chat_fullscreen(cx),
			Action::Toggle(Panel::Dock) => self.toggle_graph_fullscreen(cx),
			Action::CloseDock => {
				if self.workspace.graph_expanded {
					self.reveal_relations_after_resize();
				}

				self.workspace.graph_visible = false;
				self.workspace.graph_expanded = false;

				cx.notify();
			},
		}
	}
}
