//! Shared navigation hierarchy ink. Guides never reserve row space.
use gpui::{
	AnyElement, IntoElement, Rgba, SharedString,
	prelude::{FluentBuilder as _, InteractiveElement as _, ParentElement as _, Styled as _},
};

use super::{model::Section, ui_theme};

pub(super) fn section_color(section: Section) -> Rgba {
	let slot = match section {
		Section::Pin => 3,
		Section::Recent => 0,
		Section::Assistant => 1,
		Section::Projects => 7,
	};
	ui_theme::ColorScheme::current().navigation_slot(slot).ink
}

pub(super) fn item_color(section: &str) -> Option<Rgba> {
	if let Some(id) = section.strip_prefix("project-") {
		return Some(ui_theme::ColorScheme::current().navigation_project(id).ink);
	}
	[Section::Pin, Section::Recent, Section::Assistant]
		.into_iter()
		.find(|value| value.key() == section)
		.map(section_color)
}

pub(super) fn section_guide(key: &str, content: AnyElement, populated: bool) -> AnyElement {
	gpui::div()
		.relative()
		.child(content)
		.when(populated, |group| {
			group.child(vertical(
				format!("navigation-guide-{key}"),
				2.,
				0.,
				ui_theme::CONTROL_SIZE / 2.,
				item_color(key)
					.unwrap_or_else(|| ui_theme::rgb(ui_theme::TEXT_MUTED))
					.opacity(0.42),
			))
		})
		.into_any_element()
}

pub(super) fn project_guides(id: &str, content: AnyElement, last: bool, color: Rgba) -> AnyElement {
	gpui::div()
		.relative()
		.child(content)
		.child(vertical(
			format!("navigation-project-parent-{id}"),
			0.,
			0.,
			if last { ui_theme::CONTROL_SIZE / 2. } else { 0. },
			ui_theme::rgb(ui_theme::TEXT_MUTED).opacity(0.26),
		))
		.child(vertical(
			format!("navigation-project-guide-{id}"),
			4.,
			ui_theme::CONTROL_SIZE / 2.,
			ui_theme::CONTROL_SIZE / 2.,
			color.opacity(0.52),
		))
		.into_any_element()
}

pub(super) fn connector(key: &str, project: bool, color: Rgba) -> AnyElement {
	gpui::div()
		.id(SharedString::from(format!("navigation-connector-{key}")))
		.absolute()
		.left(gpui::px(if project { 4. } else { 2. }))
		.top(gpui::px(ui_theme::CONTROL_SIZE / 2.))
		.w(gpui::px(if project { 3. } else { 4. }))
		.h(gpui::px(1.))
		.bg(color.opacity(0.45))
		.into_any_element()
}

pub(super) fn project_header_connector() -> AnyElement {
	gpui::div()
		.absolute()
		.left_0()
		.top(gpui::px(ui_theme::CONTROL_SIZE / 2.))
		.w(gpui::px(7.))
		.h(gpui::px(1.))
		.bg(ui_theme::rgb(ui_theme::TEXT_MUTED).opacity(0.26))
		.into_any_element()
}

fn vertical(id: String, x: f32, top: f32, bottom: f32, color: Rgba) -> AnyElement {
	gpui::div()
		.debug_selector(move || id.clone())
		.absolute()
		.left(gpui::px(x))
		.top(gpui::px(top))
		.bottom(gpui::px(bottom))
		.w(gpui::px(1.))
		.bg(color)
		.into_any_element()
}
