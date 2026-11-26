//! Semantic ink for global navigation destinations.
use super::{model::Section, ui_theme};
use gpui::Rgba;

pub(super) fn section_color(section: Section) -> Rgba {
	let slot = match section {
		Section::Pin => 3,
		Section::Assistant => 0,
		Section::Projects => 1,
		Section::Chats => 2,
	};
	ui_theme::ColorScheme::current().navigation_slot(slot)
}
