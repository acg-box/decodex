//! Code keeps its own horizontal viewport; its toolbar stays with the message.
use gpui::AnyElement;

use crate::{
	shell::agent_surface::{
		InteractiveElement as _,
		markdown::{
			self, IntoElement as _, Node, ParentElement as _, SharedString,
			StatefulInteractiveElement as _, Styled as _,
			selectable_text::Document,
			ui_theme::{
				BODY_LINE_HEIGHT, CAPTION_SIZE, FONT_FAMILY, PANEL_HEADER_TINT, TEXT, TEXT_MUTED,
			},
		},
	},
	ui_theme,
};

pub(super) fn render(
	children: &[Node],
	language: Option<&str>,
	key: &str,
	selection: &mut Document,
) -> AnyElement {
	let viewport = format!("code-viewport-{key}");

	gpui::div()
		.w_full()
		.min_w_0()
		.flex()
		.flex_col()
		.rounded(gpui::px(10.))
		.border_1()
		.border_color(ui_theme::rgba(0xffffff12))
		.bg(ui_theme::rgba(0x00000030))
		.overflow_hidden()
		.child(
			gpui::div()
				.w_full()
				.h(gpui::px(32.))
				.flex_none()
				.px(gpui::px(12.))
				.flex()
				.items_center()
				.justify_between()
				.gap(gpui::px(8.))
				.bg(ui_theme::rgba(PANEL_HEADER_TINT))
				.border_b_1()
				.border_color(ui_theme::rgba(0xffffff0c))
				.font_family(FONT_FAMILY)
				.text_size(gpui::px(CAPTION_SIZE))
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.child(
					gpui::div().min_w_0().truncate().child(language.unwrap_or("text").to_owned()),
				)
				.child(markdown::copy_button(
					&format!("copy-code-{key}"),
					"Copy code",
					markdown::code_text(children),
				)),
		)
		.child(
			gpui::div()
				.id(SharedString::from(viewport.clone()))
				.debug_selector(move || viewport.clone())
				.w_full()
				.min_w_0()
				.p(gpui::px(12.))
				.flex()
				.flex_col()
				.items_start()
				.overflow_x_scroll()
				.whitespace_nowrap()
				.font_family("Menlo")
				.text_size(gpui::px(12.))
				.line_height(gpui::px(BODY_LINE_HEIGHT))
				.text_color(ui_theme::rgb(TEXT))
				.child(markdown::inline(children, key, selection)),
		)
		.into_any_element()
}

#[cfg(test)]
#[path = "agent_markdown_code_tests.rs"]
mod tests;
