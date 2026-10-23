//! Compact source activity beside the character, using the shared native symbols.
use super::{
	super::{super::activity::OperationKind, card},
	stream,
};
use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_motion, ui_theme,
};
use gpui::{App, InteractiveElement as _, ParentElement as _, Styled as _, Window};

pub(super) struct Excerpt<'a> {
	pub text: &'a str,
	pub lines: usize,
}

/// Use the same native font and word boundaries as the painted text. The graph
/// reserves only the lines present now, rather than a fixed message rectangle.
fn excerpt<'a>(
	text: &'a str,
	layout: card::Layout,
	width: f32,
	text_system: &std::sync::Arc<gpui::TextSystem>,
) -> Excerpt<'a> {
	if text.is_empty() {
		return Excerpt { text, lines: 0 };
	}
	let mut wrapper = text_system
		.line_wrapper(gpui::font(ui_theme::FONT_FAMILY), gpui::px(layout.activity_size()));
	let fragments = [gpui::LineFragment::text(text)];
	let lines = 1 + wrapper
		.wrap_line(&fragments, gpui::px(width), gpui::IndentAdjustment::NoIndent)
		.count();
	Excerpt { text, lines }
}

pub(super) fn task_excerpt<'a>(
	text: &'a str,
	layout: card::Layout,
	text_system: &std::sync::Arc<gpui::TextSystem>,
) -> Excerpt<'a> {
	excerpt(text, layout, layout.details_width(), text_system)
}

pub(super) fn message_excerpt<'a>(
	text: &'a str,
	layout: card::Layout,
	text_system: &std::sync::Arc<gpui::TextSystem>,
) -> Excerpt<'a> {
	excerpt(action(text).1, layout, layout.text_width(), text_system)
}

pub(super) fn message_height(lines: usize, layout: card::Layout) -> f32 {
	if lines == 0 {
		return 0.;
	}
	// Header, body and insets are measured in the same line units as the graph.
	(2. * layout.bubble_padding() + 2. + (1 + lines) as f32 * layout.activity_line()) * stream::LINE
		/ layout.activity_line()
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
	live: bool,
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
		.flex_none();
	for line in frame.lines {
		let (symbol, title, color) = match line.kind {
			OperationKind::Question => (Symbol::BellAttention, "Needs you", ui_theme::AMBER),
			OperationKind::Result => (Symbol::Confirm, "Result", ui_theme::GREEN),
			OperationKind::Message =>
				(Symbol::Document, if live { "Update" } else { "Last message" }, ui_theme::BLUE),
			OperationKind::Action =>
				(action(&line.text).0, if live { "Now" } else { "Last action" }, ui_theme::BLUE),
		};
		let excerpt = message_excerpt(&line.text, layout, cx.text_system());
		let selector = format!("relation-operation-{key}-{}", line.id);
		let body_selector = format!("relation-message-body-{key}");
		rows = rows.child(
			gpui::div()
				.debug_selector(move || selector.clone())
				.absolute()
				.left_0()
				.right_0()
				.top(gpui::px(line.y * scale))
				.h(gpui::px(line.height * scale))
				.child(bubble_shape(layout))
				.p(gpui::px(layout.bubble_padding()))
				.flex()
				.flex_col()
				.gap(gpui::px(2.))
				.text_size(gpui::px(layout.activity_size()))
				.line_height(gpui::px(layout.activity_line()))
				.text_color(ui_theme::rgb(ui_theme::foreground::PRIMARY))
				.opacity(line.opacity)
				.child(
					gpui::div()
						.flex()
						.items_center()
						.gap(gpui::px(4.))
						.h(gpui::px(layout.activity_line()))
						.flex_none()
						.text_color(ui_theme::rgb(color))
						.child(workspace_symbols::icon_sized(symbol, layout.pixel(11.)))
						.child(title),
				)
				.child(
					gpui::div()
						.debug_selector(move || body_selector.clone())
						.min_w_0()
						.w_full()
						.h(gpui::px(excerpt.lines as f32 * layout.activity_line()))
						.child(excerpt.text.to_owned()),
				),
		);
	}
	rows
}

pub(super) fn goal(
	key: &str,
	text: &str,
	layout: card::Layout,
	system: &std::sync::Arc<gpui::TextSystem>,
) -> gpui::Div {
	let excerpt = task_excerpt(text, layout, system);
	gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-assignment-{key}")
		})
		.w_full()
		.h(gpui::px(layout.activity_line() * excerpt.lines as f32))
		.flex_none()
		.text_size(gpui::px(layout.activity_size()))
		.line_height(gpui::px(layout.activity_line()))
		.text_color(ui_theme::rgb(ui_theme::foreground::PRIMARY))
		.child(excerpt.text.to_owned())
}

/// One quiet foreground shape. The small tail points back to the character;
/// it has no hover state, elevation, blur, or separate material.
fn bubble_shape(layout: card::Layout) -> gpui::Div {
	gpui::div().absolute().inset_0().child(
		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, _| {
				let w = f32::from(bounds.size.width);
				let h = f32::from(bounds.size.height);
				let r = layout.spacing(7.).max(5.);
				let k = r * 0.552_284_8;
				let p = |x, y| bounds.origin + gpui::point(gpui::px(x), gpui::px(y));
				let mut path = gpui::PathBuilder::fill();
				path.move_to(p(r, 0.));
				path.line_to(p(w - r, 0.));
				path.cubic_bezier_to(p(w, r), p(w - r + k, 0.), p(w, r - k));
				path.line_to(p(w, h - r));
				path.cubic_bezier_to(p(w - r, h), p(w, h - r + k), p(w - r + k, h));
				path.line_to(p(r, h));
				path.cubic_bezier_to(p(0., h - r), p(r - k, h), p(0., h - r + k));
				path.line_to(p(0., r + 7.));
				path.line_to(p(-4., r + 3.));
				path.line_to(p(0., r + 1.));
				path.line_to(p(0., r));
				path.cubic_bezier_to(p(r, 0.), p(0., r - k), p(r - k, 0.));
				path.close();
				if let Ok(path) = path.build() {
					window.paint_path(path, ui_theme::rgba(ui_theme::HOVER_FILL));
				}
			},
		)
		.size_full(),
	)
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
	fn complete_thoughts_wrap_without_losing_their_ending_at_any_readable_scale(
		cx: &mut gpui::TestAppContext,
	) {
		let system = cx.update(|cx| cx.text_system().clone());
		for zoom in [0.55, 0.64, 0.75, 1.] {
			let layout = card::Layout::new(zoom).on_display(2.);
			assert!(layout.text_width() >= 90.);
			assert_eq!(message_excerpt("Done", layout, &system).lines, 1);
			let text = "Compare the recovered answer with the original response, then verify that the final paragraph and all code blocks are complete.";
			let message = message_excerpt(text, layout, &system);
			assert_eq!(message.text, text);
			assert!(message.lines >= 3);
			assert!(!message.text.ends_with('…'));
			assert_eq!(task_excerpt(text, layout, &system).text, text);
			let cjk = "检查恢复后的完整答复，并确认最后一段和代码块都没有丢失。".repeat(3);
			assert_eq!(message_excerpt(&cjk, layout, &system).text, cjk);
		}
		let layout = card::Layout::new(1.).on_display(2.);
		let excerpt = message_excerpt(
			"All checks passed. There are 47 detailed observations in the report.",
			layout,
			&system,
		);
		assert_eq!(
			excerpt.text,
			"All checks passed. There are 47 detailed observations in the report."
		);
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
