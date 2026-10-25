//! Compact speech above the character, using the shared native symbols.
use super::{
	super::{super::activity::OperationKind, card},
	stream,
};
use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_motion, ui_theme,
};
use gpui::{
	App, InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
	Window,
};

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
	excerpt(text, layout, layout.caption_width(), text_system)
}

pub(super) fn message_excerpt<'a>(
	text: &'a str,
	layout: card::Layout,
	text_system: &std::sync::Arc<gpui::TextSystem>,
) -> Excerpt<'a> {
	// The icon reinforces the source action; it must not replace its verb.
	// A filename alone cannot distinguish reading, editing and running it.
	excerpt(text, layout, layout.text_width(), text_system)
}

pub(super) fn message_height(lines: usize, layout: card::Layout) -> f32 {
	if lines == 0 {
		return 0.;
	}
	// The semantic icon shares the first text line; it does not reserve a label row.
	(2. * layout.bubble_padding() + lines as f32 * layout.activity_line()) * stream::LINE
		/ layout.activity_line()
}

pub(super) fn bubble_width(
	excerpt: &Excerpt<'_>,
	layout: card::Layout,
	system: &gpui::WindowTextSystem,
) -> f32 {
	if excerpt.lines != 1 || excerpt.text.contains('\n') {
		return layout.details_width();
	}
	let line = system.shape_line(
		excerpt.text.to_owned().into(),
		gpui::px(layout.activity_size()),
		&[gpui::TextRun {
			len: excerpt.text.len(),
			font: gpui::font(ui_theme::FONT_FAMILY),
			..Default::default()
		}],
		None,
	);
	(f32::from(line.width).ceil() + 1. + 2. * layout.bubble_padding() + layout.title_size() + 5.)
		.min(layout.details_width())
}

fn action_icon(text: &str) -> Symbol {
	let kind = text.split_once(" · ").map_or(text, |(kind, _)| kind);
	match kind {
		"Plan" => Symbol::ActivityGoal,
		"Read" | "Reading file" => Symbol::Document,
		"Find" | "Search" | "Searching the web" => Symbol::ActivitySearch,
		"Edit" | "Applying changes" => Symbol::ActivityEdit,
		"Run" | "Running command" | "Command" => Symbol::ActivityCommand,
		"Test" | "Done" => Symbol::Confirm,
		"Review" => Symbol::Eye,
		"Send" => Symbol::ActivityHandoff,
		_ => Symbol::Document,
	}
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
	let target_width = frame.lines.last().map_or(0., |line| {
		bubble_width(
			&message_excerpt(&line.text, layout, cx.text_system()),
			layout,
			window.text_system(),
		)
	});
	let width = ui_motion::value(
		gpui::SharedString::from(format!("relation-speech-width-{key}")),
		target_width,
		window,
		cx,
	);
	let left = (layout.details_width() - width) / 2.;
	let mut rows = gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-preview-{key}")
		})
		.relative()
		.w_full()
		.h(gpui::px(frame.height * scale))
		.flex_none()
		.child(
			gpui::div()
				.absolute()
				.left(gpui::px(left))
				.top_0()
				.w(gpui::px(width))
				.h(gpui::px(frame.height * scale))
				.child(bubble_shape(layout)),
		);
	for line in frame.lines {
		let (symbol, title, color) = match line.kind {
			OperationKind::Question => (Symbol::BellAttention, "Needs you", ui_theme::AMBER),
			OperationKind::Result => (Symbol::Confirm, "Result", ui_theme::GREEN),
			OperationKind::Message =>
				(Symbol::Document, if live { "Update" } else { "Last message" }, ui_theme::BLUE),
			OperationKind::Action =>
				(action_icon(&line.text), if live { "Now" } else { "Last action" }, ui_theme::BLUE),
		};
		let excerpt = message_excerpt(&line.text, layout, cx.text_system());
		let selector = format!("relation-operation-{key}-{}", line.id);
		let body_selector = format!("relation-message-body-{key}");
		rows = rows.child(
			gpui::div()
				.id(gpui::SharedString::from(selector.clone()))
				.debug_selector(move || selector.clone())
				.role(gpui::Role::Group)
				.aria_label(format!("{title}: {}", excerpt.text))
				.absolute()
				.left(gpui::px(left))
				.w(gpui::px(width))
				.top(gpui::px(line.y * scale))
				.h(gpui::px(frame.height * scale))
				.overflow_hidden()
				.p(gpui::px(layout.bubble_padding()))
				.flex()
				.items_start()
				.gap(gpui::px(5.))
				.text_size(gpui::px(layout.activity_size()))
				.line_height(gpui::px(layout.activity_line()))
				.text_color(ui_theme::rgb(ui_theme::foreground::PRIMARY))
				.opacity(line.opacity)
				.child(
					gpui::div()
						.mt(gpui::px(
							layout.pixel((layout.activity_line() - layout.title_size()) / 2.),
						))
						.size(gpui::px(layout.title_size()))
						.flex_none()
						.text_color(ui_theme::rgb(color))
						.child(workspace_symbols::icon_sized(symbol, layout.title_size())),
				)
				.child(
					gpui::div()
						.debug_selector(move || body_selector.clone())
						.min_w_0()
						.flex_1()
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
		.text_color(ui_theme::rgb(ui_theme::foreground::BODY))
		.text_center()
		.child(excerpt.text.to_owned())
}

/// One quiet foreground shape. Its lower tail points down to the character;
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
				let tip_y = layout.speech_tip(h).1;
				let p = |x, y| bounds.origin + gpui::point(gpui::px(x), gpui::px(y));
				let mut path = gpui::PathBuilder::fill();
				path.move_to(p(r, 0.));
				path.line_to(p(w - r, 0.));
				path.cubic_bezier_to(p(w, r), p(w - r + k, 0.), p(w, r - k));
				path.line_to(p(w, h - r));
				path.cubic_bezier_to(p(w - r, h), p(w, h - r + k), p(w - r + k, h));
				path.line_to(p(w / 2. + 4., h));
				path.line_to(p(w / 2., h + tip_y));
				path.line_to(p(w / 2. - 4., h));
				path.line_to(p(r, h));
				path.cubic_bezier_to(p(0., h - r), p(r - k, h), p(0., h - r + k));
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
	// Spend the small badge's empty padding on a readable people silhouette.
	// Its width stays stable, including when a long agent number shares the row.
	let icon_size = layout.team_icon_size();
	gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-team-{key}")
		})
		.flex()
		.items_center()
		.gap(gpui::px(2.))
		.flex_none()
		.h(gpui::px(layout.title_line()))
		.text_size(gpui::px(layout.title_size()))
		.line_height(gpui::px(layout.title_line()))
		.font_features(card::number_features())
		.text_color(ui_theme::rgb(ui_theme::foreground::BODY))
		.child(
			gpui::div()
				.debug_selector({
					let key = key.to_owned();
					move || format!("relation-team-icon-{key}")
				})
				.relative()
				.w(gpui::px(icon_size))
				.h(gpui::px(layout.title_line()))
				.flex_none()
				.child(
					gpui::div()
						.absolute()
						.top(gpui::px((layout.title_line() - icon_size) / 2.))
						.child(workspace_symbols::icon_sized(Symbol::TeamMembers, icon_size)),
				),
		)
		.child(
			gpui::div()
				.debug_selector({
					let key = key.to_owned();
					move || format!("relation-team-count-{key}")
				})
				.child(count.to_string()),
		)
}

pub(super) fn parent(key: &str, parent: String, layout: card::Layout) -> gpui::Stateful<gpui::Div> {
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
	row.id(gpui::SharedString::from(format!("relation-parent-{key}")))
		.aria_label(format!("Reports to {parent}"))
		.child(workspace_symbols::icon_sized(Symbol::TeamParent, layout.title_size()))
		.child(parent)
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

	#[gpui::test]
	fn operation_bubbles_keep_the_source_action_and_its_complete_object(
		cx: &mut gpui::TestAppContext,
	) {
		let system = cx.update(|cx| cx.text_system().clone());
		for zoom in [1., 0.75, 0.51] {
			let layout = card::Layout::new(zoom).on_display(2.);
			for source in [
				"Read account_route.rs",
				"Custom tool · file:42",
				"Inspect · a · b",
				"Read · file:42",
				"Reading file · 恢复记录.rs",
				"Edit · file:42",
				"Run · printf a · b",
			] {
				assert_eq!(message_excerpt(source, layout, &system).text, source);
			}
		}
	}

	#[gpui::test]
	fn short_bubbles_fit_their_text_and_long_replies_use_the_reading_column(
		cx: &mut gpui::TestAppContext,
	) {
		let visual = cx.add_empty_window();
		visual.update(|window, cx| {
			let system = cx.text_system();
			for density in [1., 2.] {
				for zoom in [0.51, 0.75, 1.] {
					let layout = card::Layout::new(zoom).on_display(density);
					let short = message_excerpt("Done", layout, system);
					let width = bubble_width(&short, layout, window.text_system());
					assert!(width < layout.details_width() / 2.);
					assert_eq!(
						excerpt(
							short.text,
							layout,
							width - 2. * layout.bubble_padding() - layout.title_size() - 5.,
							system
						)
						.lines,
						1
					);
					let long = message_excerpt(
						"Verify the entire recovered response, including the final paragraph, all source links, and every code block before returning the result.",
						layout,
						system,
					);
					assert!(long.lines > 1);
					assert_eq!(bubble_width(&long, layout, window.text_system()), layout.details_width());
				}
			}
		});
	}
}
