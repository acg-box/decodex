//! Shared disclosure geometry for process steps and complete voice calls.
use crate::{
	shell::workspace_symbols,
	ui_theme::{self, TEXT, TEXT_MUTED},
};
use gpui::{
	AnyElement, Div, FontWeight, Role, SharedString, Stateful,
	prelude::{
		InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
	},
};

#[derive(Clone, Copy)]
pub(super) enum Style {
	Process,
	Section,
}
impl Style {
	pub(super) const fn height(self) -> f32 {
		match self {
			Self::Process => 20.,
			Self::Section => ui_theme::TREE_ROW_HEIGHT,
		}
	}
}

pub(super) fn control(key: String, label: String, expanded: bool, style: Style) -> Stateful<Div> {
	let (size, weight, ink, line_height, padding) = match style {
		Style::Process =>
			(ui_theme::foreground::METRIC_SIZE, FontWeight::NORMAL, TEXT_MUTED, 14., 3.),
		Style::Section =>
			(ui_theme::BODY_SIZE, FontWeight::SEMIBOLD, TEXT, ui_theme::BODY_LINE_HEIGHT, 2.),
	};
	gpui::div()
		.id(SharedString::from(key.clone()))
		.role(Role::Button)
		.tab_index(0)
		.aria_expanded(expanded)
		.aria_label(label.clone())
		.cursor_pointer()
		.flex()
		.items_center()
		.flex_none()
		.h(gpui::px(style.height()))
		.gap(gpui::px(6.))
		.py(gpui::px(padding))
		.text_size(gpui::px(size))
		.font_weight(weight)
		.line_height(gpui::px(line_height))
		.whitespace_nowrap()
		.text_color(ui_theme::rgb(ink))
		.hover(|s| s.text_color(ui_theme::rgb(TEXT)))
		.child(workspace_symbols::process_chevron(
			SharedString::from(format!("{key}-chevron")),
			expanded,
		))
		.child(label)
}

pub(super) fn row(control: AnyElement, style: Style) -> Div {
	gpui::div()
		.w_full()
		.min_w_0()
		.h(gpui::px(style.height()))
		.flex()
		.items_center()
		.gap(gpui::px(12.))
		.child(control)
		.child(gpui::div().min_w_0().flex_1().max_w(gpui::px(160.)).h(gpui::px(1.)).bg(
			gpui::linear_gradient(
				90.,
				gpui::linear_color_stop(ui_theme::rgb(TEXT).opacity(0.17), 0.),
				gpui::linear_color_stop(ui_theme::rgb(TEXT).opacity(0.), 1.),
			),
		))
}
