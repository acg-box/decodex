//! Compact source activity beside the character, using the shared native symbols.
use super::{super::card, stream};
use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_motion, ui_theme,
};
use gpui::{App, InteractiveElement as _, ParentElement as _, Styled as _, Window};

pub(super) const TASK_LINES: usize = 4;
pub(super) fn action_lines(current: bool) -> usize {
	if current { 6 } else { 2 }
}

/// Use the same native font and word boundaries as the painted text. The graph
/// reserves only the lines present now, rather than a fixed message rectangle.
pub(super) fn measure(
	text: &str,
	layout: card::Layout,
	limit: usize,
	text_system: &std::sync::Arc<gpui::TextSystem>,
) -> usize {
	if text.is_empty() {
		return 0;
	}
	let mut wrapper = text_system
		.line_wrapper(gpui::font(ui_theme::FONT_FAMILY), gpui::px(layout.activity_size()));
	let fragments = [gpui::LineFragment::text(text)];
	(1 + wrapper
		.wrap_line(&fragments, gpui::px(layout.text_width()), gpui::IndentAdjustment::NoIndent)
		.count())
	.min(limit)
}

pub(super) fn action(text: &str) -> (Symbol, &str) {
	let Some((kind, body)) = text.split_once(" · ") else { return (Symbol::Document, text) };
	let icon = match kind {
		"Plan" => Symbol::ActivityGoal,
		"Read" | "Reading file" => Symbol::Document,
		"Find" | "Search" | "Searching the web" => Symbol::ActivitySearch,
		"Edit" | "Applying changes" => Symbol::ActivityEdit,
		"Run" | "Running command" => Symbol::ActivityCommand,
		"Test" | "Done" => Symbol::Confirm,
		"Review" => Symbol::Eye,
		"Send" => Symbol::ActivityHandoff,
		_ => return (Symbol::Document, text),
	};
	(icon, body)
}

pub(super) fn render(
	key: &str,
	frame: stream::Frame,
	layout: card::Layout,
	window: &mut Window,
	cx: &mut App,
) -> gpui::Div {
	let scale = layout.activity_line() / stream::LINE;
	if frame.moving {
		ui_motion::request_frame(window, cx);
	}
	let mut rows = gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-preview-{key}")
		})
		.relative()
		.w_full()
		.h(gpui::px(frame.height * scale))
		.flex_none()
		.overflow_hidden();
	for line in frame.lines {
		let (symbol, body) = action(&line.text);
		let selector = format!("relation-operation-{key}-{}", line.id);
		rows = rows.child(
			gpui::div()
				.debug_selector(move || selector.clone())
				.absolute()
				.left_0()
				.right_0()
				.top(gpui::px(line.y * scale))
				.h(gpui::px(line.height * scale))
				.flex()
				.items_start()
				.gap(gpui::px(5.))
				.text_size(gpui::px(layout.activity_size()))
				.line_height(gpui::px(layout.activity_line()))
				.text_color(ui_theme::rgb(if line.current {
					ui_theme::foreground::PRIMARY
				} else {
					ui_theme::foreground::BODY
				}))
				.opacity(line.opacity)
				.child(workspace_symbols::icon_sized(symbol, layout.pixel(12.)))
				.child(
					gpui::div()
						.flex_1()
						.min_w_0()
						.line_clamp(action_lines(line.current))
						.overflow_hidden()
						.child(body.to_owned()),
				),
		);
	}
	rows
}

pub(super) fn goal(key: &str, text: String, layout: card::Layout, lines: usize) -> gpui::Div {
	gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-assignment-{key}")
		})
		.w_full()
		.h(gpui::px(layout.activity_line() * lines as f32))
		.flex_none()
		.flex()
		.items_start()
		.gap(gpui::px(5.))
		.text_size(gpui::px(layout.activity_size()))
		.line_height(gpui::px(layout.activity_line()))
		.text_color(ui_theme::rgb(ui_theme::foreground::BODY))
		.child(workspace_symbols::icon_sized(Symbol::ActivityGoal, layout.pixel(12.)))
		.child(gpui::div().flex_1().min_w_0().line_clamp(TASK_LINES).overflow_hidden().child(text))
}

pub(super) fn team(key: &str, count: usize, layout: card::Layout) -> gpui::Div {
	gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-team-{key}")
		})
		.flex()
		.items_center()
		.gap(gpui::px(3.))
		.ml(gpui::px(6.))
		.flex_none()
		.h(gpui::px(layout.title_line()))
		.text_size(gpui::px(layout.pixel((11.5 * layout.zoom).max(11.))))
		.font_features(card::number_features())
		.text_color(ui_theme::rgb(ui_theme::foreground::BODY))
		.child(workspace_symbols::icon_sized(
			Symbol::TeamMembers,
			layout.pixel((14. * layout.zoom).max(13.)),
		))
		.child(count.to_string())
}

pub(super) fn parent(key: &str, parent: String, layout: card::Layout) -> gpui::Div {
	let row = gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-owner-hint-{key}")
		})
		.flex()
		.items_center()
		.gap(gpui::px(3.))
		.ml(gpui::px(4.))
		.flex_none()
		.h(gpui::px(layout.title_line()))
		.text_size(gpui::px(layout.title_size()))
		.text_color(ui_theme::rgb(ui_theme::foreground::BODY));
	if layout.zoom >= 0.9 {
		row.child(format!("Reports to {parent}"))
	} else {
		row.child(workspace_symbols::icon_sized(Symbol::TeamParent, 11.)).child(parent)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[gpui::test]
	fn wrapped_messages_use_native_font_widths_at_each_readable_scale(
		cx: &mut gpui::TestAppContext,
	) {
		let system = cx.update(|cx| cx.text_system().clone());
		for zoom in [0.55, 0.64, 0.75, 1.] {
			let layout = card::Layout::new(zoom).on_display(2.);
			assert!(layout.text_width() >= 90.);
			assert_eq!(measure("Done", layout, 6, &system), 1);
			let text = "Compare the recovered answer with the original response, then verify that the final paragraph and all code blocks are complete.";
			assert!(measure(text, layout, 6, &system) > 2);
			assert_eq!(measure(text, layout, 2, &system), 2);
			assert!(
				measure(
					&"检查恢复后的完整答复，并确认最后一段和代码块都没有丢失。".repeat(3),
					layout,
					6,
					&system
				) > 1
			);
		}
	}

	#[test]
	fn unknown_labels_preserve_the_complete_source_text() {
		for source in ["Read account_route.rs", "Custom tool · file:42", "Inspect · a · b"] {
			assert_eq!(action(source).1, source);
		}
		assert_eq!(action("Read · file:42").1, "file:42");
		assert_eq!(action("Run · printf a · b").1, "printf a · b");
	}
}
