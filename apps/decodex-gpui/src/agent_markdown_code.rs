//! Code keeps its own horizontal viewport; its toolbar stays with the message.
use crate::ui_theme::{rgb, rgba};
use gpui::{AnyElement, px};

use super::{
	IntoElement as _, Node, ParentElement as _, SharedString, StatefulInteractiveElement as _,
	Styled as _, code_text, copy_button, inline, selectable_text::Document, ui_theme,
};
use crate::shell::agent_surface::InteractiveElement as _;

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
		.rounded(px(10.))
		.border_1()
		.border_color(rgba(0xffffff12))
		.bg(rgba(0x00000030))
		.overflow_hidden()
		.child(
			gpui::div()
				.w_full()
				.h(px(32.))
				.flex_none()
				.px(px(12.))
				.flex()
				.items_center()
				.justify_between()
				.gap(px(8.))
				.bg(rgba(ui_theme::PANEL_HEADER_TINT))
				.border_b_1()
				.border_color(rgba(0xffffff0c))
				.font_family(ui_theme::FONT_FAMILY)
				.text_size(px(ui_theme::CAPTION_SIZE))
				.text_color(rgb(ui_theme::TEXT_MUTED))
				.child(
					gpui::div().min_w_0().truncate().child(language.unwrap_or("text").to_owned()),
				)
				.child(copy_button(&format!("copy-code-{key}"), "Copy code", code_text(children))),
		)
		.child(
			gpui::div()
				.id(SharedString::from(viewport.clone()))
				.debug_selector(move || viewport.clone())
				.w_full()
				.min_w_0()
				.p(px(12.))
				.flex()
				.flex_col()
				.items_start()
				.overflow_x_scroll()
				.whitespace_nowrap()
				.font_family("Menlo")
				.text_size(px(12.))
				.line_height(px(ui_theme::BODY_LINE_HEIGHT))
				.text_color(rgb(ui_theme::TEXT))
				.child(inline(children, key, selection)),
		)
		.into_any_element()
}

#[cfg(test)]
#[path = "agent_markdown_code_tests.rs"]
mod tests;
