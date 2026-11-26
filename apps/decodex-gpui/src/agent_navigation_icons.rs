//! One outline family for navigation, scoped creation and row actions.
use std::{f32::consts::TAU, time::Instant};

use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_motion::{self, AgentStatusStyle},
	ui_theme,
};
use gpui::{
	App, IntoElement, RenderOnce, Rgba, Role, SharedString, Window,
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
	Chats,
	Sort,
	Plus,
	Search,
	Conversations,
	Archive,
	Trash,
	Back,
}
#[derive(IntoElement)]
pub(super) struct Icon {
	pub glyph: Glyph,
	pub size: f32,
}
impl RenderOnce for Icon {
	fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
		let symbol = match self.glyph {
			Glyph::Pin => Symbol::NavigationPin,
			Glyph::Assistant => Symbol::NavigationAssistant,
			Glyph::Folder => Symbol::NavigationFolder,
			Glyph::Projects => Symbol::NavigationProjects,
			Glyph::Chats => Symbol::NavigationChats,
			Glyph::Sort => Symbol::NavigationSort,
			Glyph::Plus => Symbol::Plus,
			Glyph::Search => Symbol::ActivitySearch,
			Glyph::Archive => Symbol::Archive,
			Glyph::Conversations => Symbol::Document,
			Glyph::Trash => Symbol::Trash,
			Glyph::Back => Symbol::ChevronLeft,
		};
		workspace_symbols::icon_sized(symbol, self.size)
	}
}

#[derive(IntoElement)]
pub(super) struct ConversationMark {
	pub id: String,
	pub state: String,
	pub style: AgentStatusStyle,
	pub label: String,
	pub compact: bool,
}
impl RenderOnce for ConversationMark {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let ink = if self.compact {
			let style = if self.style == AgentStatusStyle::Completed {
				AgentStatusStyle::Quiet
			} else {
				self.style
			};
			ui_motion::agent_state_ink(&self.id, style, window, cx)
		} else {
			ui_theme::rgb(self.style.color())
		};
		let mark = if self.compact {
			ui_motion::RailInitial { id: format!("initial-{}", self.id), label: self.label }
				.into_any_element()
		} else {
			let size = ui_theme::NAVIGATION_ACTION_SIZE;
			let indicator = match self.style {
				AgentStatusStyle::Running =>
					Some(activity_dots(&self.id, ink, window, cx).into_any_element()),
				AgentStatusStyle::Attention
				| AgentStatusStyle::Failed
				| AgentStatusStyle::UnreadResult => {
					let circle = gpui::div().size(gpui::px(6.)).rounded_full();
					Some(if self.style == AgentStatusStyle::UnreadResult {
						circle.bg(ink).into_any_element()
					} else {
						circle.border_1().border_color(ink).into_any_element()
					})
				},
				AgentStatusStyle::Quiet | AgentStatusStyle::Completed => None,
			};
			gpui::div()
				.size(gpui::px(size))
				.flex()
				.items_center()
				.justify_center()
				.children(indicator)
				.into_any_element()
		};
		let id = SharedString::from(self.id);
		gpui::div()
			.id(id.clone())
			.debug_selector(move || format!("{id}-mark"))
			.role(Role::Status)
			.aria_label(self.state)
			.text_color(ink)
			.child(mark)
	}
}

fn activity_dots(id: &str, ink: Rgba, window: &mut Window, cx: &mut App) -> gpui::Div {
	let phase = (!ui_motion::reduced()).then(|| {
		let clock = window.use_keyed_state(
			SharedString::from(format!("sidebar-activity-{id}")),
			cx,
			|_, _| Instant::now(),
		);
		ui_motion::request_frame(window, cx);
		clock.read(cx).elapsed().as_secs_f32() * TAU / 1.2
	});
	gpui::div().flex().items_center().gap(gpui::px(1.5)).children((0..3).map(|index| {
		let opacity = phase.map_or(1., |phase| {
			let pulse = (phase - index as f32 * TAU / 3.).cos();
			0.66 + 0.34 * pulse
		});
		gpui::div().size(gpui::px(2.5)).rounded_full().bg(ink.opacity(opacity))
	}))
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
