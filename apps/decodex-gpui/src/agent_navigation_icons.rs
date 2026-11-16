//! One outline family for navigation, scoped creation and row actions.
use gpui::{
	App, IntoElement, RenderOnce, Role, Window,
	prelude::{
		InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
	},
};
#[derive(Clone, Copy)]
pub(super) enum Glyph {
	Pin,
	Assistant,
	Folder,
	Projects,
	Sort,
	Plus,
	Search,
	Archive,
	Trash,
	Restore,
	Back,
}
#[derive(IntoElement)]
pub(super) struct Icon {
	pub glyph: Glyph,
	pub size: f32,
}
impl RenderOnce for Icon {
	fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
		use crate::shell::workspace_symbols::{self, Symbol};
		let symbol = match self.glyph {
			Glyph::Pin => Symbol::NavigationPin,
			Glyph::Assistant => Symbol::NavigationAssistant,
			Glyph::Folder => Symbol::NavigationFolder,
			Glyph::Projects => Symbol::NavigationProjects,
			Glyph::Sort => Symbol::NavigationSort,
			Glyph::Plus => Symbol::Plus,
			Glyph::Search => Symbol::ActivitySearch,
			Glyph::Archive => Symbol::Archive,
			Glyph::Trash => Symbol::Trash,
			Glyph::Restore => Symbol::Unarchive,
			Glyph::Back => Symbol::ChevronLeft,
		};
		workspace_symbols::icon_sized(symbol, self.size)
	}
}

#[derive(IntoElement)]
pub(super) struct ConversationIcon {
	pub id: String,
	pub state: String,
	pub label: String,
	pub compact: bool,
}
impl RenderOnce for ConversationIcon {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		use crate::shell::workspace_symbols::{self, Symbol};
		let ink = crate::ui_motion::agent_state_ink(&self.id, &self.state, window, cx);
		let mark = if self.compact {
			crate::ui_motion::RailInitial { id: format!("initial-{}", self.id), label: self.label }
				.into_any_element()
		} else {
			workspace_symbols::icon_sized(
				Symbol::Conversation,
				crate::ui_theme::NAVIGATION_ACTION_SIZE,
			)
		};
		gpui::div()
			.id(gpui::SharedString::from(self.id))
			.role(Role::Status)
			.aria_label(self.state)
			.text_color(ink)
			.child(mark)
	}
}

#[derive(IntoElement)]
pub(super) struct ProjectIcon {
	pub id: String,
	pub label: String,
	pub compact: bool,
}
impl RenderOnce for ProjectIcon {
	fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
		if self.compact {
			crate::ui_motion::RailInitial {
				id: format!("initial-project-{}", self.id),
				label: self.label,
			}
			.into_any_element()
		} else {
			Icon { glyph: Glyph::Folder, size: crate::ui_theme::NAVIGATION_ICON_SIZE }
				.into_any_element()
		}
	}
}
