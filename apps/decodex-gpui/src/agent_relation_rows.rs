//! Compact speech above the character, using the shared native symbols.
use gpui::{
	App, Div, InteractiveElement as _, ParentElement as _, PathBuilder, Role, SharedString,
	Stateful, StatefulInteractiveElement as _, Styled as _, TextRun, Window, WindowTextSystem,
};

use crate::{
	shell::{
		agent_surface::work_board::{
			activity::OperationKind,
			relations::{
				card,
				card::Layout,
				header::stream::{Frame, LINE},
			},
		},
		workspace_symbols::{self, Symbol},
	},
	ui_motion,
	ui_theme::{
		self, BLUE, FONT_FAMILY, HOVER_FILL, SUCCESS, WARNING,
		foreground::{BODY, PRIMARY, SECONDARY},
	},
};

pub(super) struct Excerpt<'a> {
	pub text: &'a str,
	pub lines: usize,
}

pub(super) fn task_excerpt<'a>(
	text: &'a str,
	layout: Layout,
	text_system: &WindowTextSystem,
) -> Excerpt<'a> {
	excerpt(text, layout, layout.caption_width(), text_system)
}

pub(super) fn message_excerpt<'a>(
	text: &'a str,
	layout: Layout,
	text_system: &WindowTextSystem,
) -> Excerpt<'a> {
	// The icon reinforces the source action; it must not replace its verb.
	// A filename alone cannot distinguish reading, editing and running it.
	excerpt(text, layout, layout.text_width(), text_system)
}

pub(super) fn message_height(lines: usize, layout: Layout) -> f32 {
	if lines == 0 {
		return 0.;
	}

	// The semantic icon shares the first text line; it does not reserve a label row.
	(2. * layout.bubble_padding() + lines as f32 * layout.activity_line()) * LINE
		/ layout.activity_line()
}

pub(super) fn bubble_width(
	excerpt: &Excerpt<'_>,
	layout: Layout,
	system: &WindowTextSystem,
) -> f32 {
	if excerpt.lines != 1 || excerpt.text.contains('\n') {
		return layout.details_width();
	}

	let line = system.shape_line(
		excerpt.text.to_owned().into(),
		gpui::px(layout.activity_size()),
		&[TextRun { len: excerpt.text.len(), font: gpui::font(FONT_FAMILY), ..Default::default() }],
		None,
	);

	(f32::from(line.width).ceil() + 1. + 2. * layout.bubble_padding() + layout.title_size() + 5.)
		.min(layout.details_width())
}

pub(super) fn render(
	key: &str,
	frame: Frame,
	layout: Layout,
	live: bool,
	window: &mut Window,
	cx: &mut App,
) -> Div {
	let scale = layout.activity_line() / LINE;
	let target_width = frame.lines.last().map_or(0., |line| {
		bubble_width(
			&message_excerpt(&line.text, layout, window.text_system()),
			layout,
			window.text_system(),
		)
	});
	let metrics =
		(layout.zoom, layout.details_width(), layout.activity_size(), layout.bubble_padding());
	let measured = window.use_keyed_state(
		SharedString::from(format!("relation-speech-layout-{key}")),
		cx,
		|_, _| metrics,
	);
	let resized = measured.update(cx, |previous, _| {
		let resized = *previous != metrics;

		*previous = metrics;
		resized
	});
	// The camera already animates zoom. A second width tween leaves text and
	// bubble height measured at different widths until that tween catches up.
	let width = layout.pixel(ui_motion::direct_value(
		SharedString::from(format!("relation-speech-width-{key}")),
		target_width,
		resized,
		window,
		cx,
	));
	let left = layout.pixel((layout.details_width() - width) / 2.);
	let height = layout.pixel(frame.height * scale);
	let mut rows = gpui::div()
		.debug_selector({
			let key = key.to_owned();

			move || format!("relation-preview-{key}")
		})
		.relative()
		.w_full()
		.h(gpui::px(height))
		.flex_none()
		.child(
			gpui::div()
				.absolute()
				.left(gpui::px(left))
				.top_0()
				.w(gpui::px(width))
				.h(gpui::px(height))
				.child(bubble_shape(layout)),
		);

	for line in frame.lines {
		let (symbol, title, color) = match line.kind {
			OperationKind::Question => (Symbol::BellAttention, "Needs you", WARNING),
			OperationKind::Result => (Symbol::Confirm, "Result", SUCCESS),
			OperationKind::Message =>
				(Symbol::Document, if live { "Update" } else { "Last message" }, BLUE),
			OperationKind::Action =>
				(action_icon(&line.text), if live { "Now" } else { "Last action" }, BLUE),
		};
		let excerpt = message_excerpt(&line.text, layout, window.text_system());
		let selector = format!("relation-operation-{key}-{}", line.id);
		let body_selector = format!("relation-message-body-{key}");

		rows = rows.child(
			gpui::div()
				.id(SharedString::from(selector.clone()))
				.debug_selector(move || selector.clone())
				.role(Role::Group)
				.aria_label(format!("{title}: {}", excerpt.text))
				.absolute()
				.left(gpui::px(left))
				.w(gpui::px(width))
				.top(gpui::px(layout.pixel(line.y * scale)))
				.h(gpui::px(height))
				.overflow_hidden()
				.p(gpui::px(layout.bubble_padding()))
				.flex()
				.items_start()
				.gap(gpui::px(5.))
				.text_size(gpui::px(layout.activity_size()))
				.line_height(gpui::px(layout.activity_line()))
				.text_color(ui_theme::rgb(PRIMARY))
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

pub(super) fn goal(key: &str, text: &str, layout: Layout, system: &WindowTextSystem) -> Div {
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
		.text_color(ui_theme::rgb(BODY))
		.text_center()
		.child(excerpt.text.to_owned())
}

pub(super) fn team(key: &str, count: usize, layout: Layout) -> Div {
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
		.text_color(ui_theme::rgb(SECONDARY))
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

pub(super) fn parent(key: &str, parent: String, layout: Layout) -> Stateful<Div> {
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
		.text_color(ui_theme::rgb(BODY));

	row.id(SharedString::from(format!("relation-parent-{key}")))
		.aria_label(format!("Reports to {parent}"))
		.child(workspace_symbols::icon_sized(Symbol::TeamParent, layout.title_size()))
		.child(parent)
}

/// Use the same native font and word boundaries as the painted text. The graph
/// reserves only the lines present now, rather than a fixed message rectangle.
fn excerpt<'a>(
	text: &'a str,
	layout: Layout,
	width: f32,
	text_system: &WindowTextSystem,
) -> Excerpt<'a> {
	if text.is_empty() {
		return Excerpt { text, lines: 0 };
	}

	// Use the same shaped glyphs and line breaks as GPUI's text element. The
	// per-character line wrapper can estimate extra lines for the real font.
	let runs = [TextRun { len: text.len(), font: gpui::font(FONT_FAMILY), ..Default::default() }];
	let lines = text_system
		.shape_text(
			text.to_owned().into(),
			gpui::px(layout.activity_size()),
			&runs,
			Some(gpui::px(width)),
			None,
		)
		.map(|lines| lines.iter().map(|line| 1 + line.wrap_boundaries.len()).sum())
		.unwrap_or(0);

	Excerpt { text, lines }
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

/// One quiet foreground shape. Its lower tail points down to the character;
/// it has no hover state, elevation, blur, or separate material.
fn bubble_shape(layout: Layout) -> Div {
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
				let mut path = PathBuilder::fill();

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
					window.paint_path(path, ui_theme::rgba(HOVER_FILL));
				}
			},
		)
		.size_full(),
	)
}

#[cfg(test)]
mod tests {

	use crate::shell::agent_surface::work_board::relations::header::information_rows::{
		self, card,
	};

	#[gpui::test]
	fn explicit_line_breaks_count_as_visible_rows(cx: &mut gpui::TestAppContext) {
		let visual = cx.add_empty_window();

		visual.update(|window, _| {
			let layout = card::Layout::new(0.75).on_display(2.);
			let system = window.text_system();

			for (text, lines) in [("One line", 1), ("First\nSecond", 2), ("First\n\nThird", 3)] {
				assert_eq!(information_rows::message_excerpt(text, layout, system).lines, lines);
				assert_eq!(information_rows::task_excerpt(text, layout, system).lines, lines);
			}
		});
	}

	#[gpui::test]
	fn complete_thoughts_wrap_without_losing_their_ending_at_any_readable_scale(
		cx: &mut gpui::TestAppContext,
	) {
		let system = gpui::WindowTextSystem::new(cx.update(|cx| cx.text_system().clone()));

		for zoom in [0.55, 0.64, 0.75, 1.] {
			let layout = card::Layout::new(zoom).on_display(2.);

			assert!(layout.text_width() >= 90.);
			assert_eq!(information_rows::message_excerpt("Done", layout, &system).lines, 1);

			let text = "Compare the recovered answer with the original response, then verify that the final paragraph and all code blocks are complete.";
			let message = information_rows::message_excerpt(text, layout, &system);

			assert_eq!(message.text, text);
			assert!(message.lines >= 3);
			assert!(!message.text.ends_with('…'));
			assert_eq!(information_rows::task_excerpt(text, layout, &system).text, text);

			let cjk = "检查恢复后的完整答复，并确认最后一段和代码块都没有丢失。".repeat(3);

			assert_eq!(information_rows::message_excerpt(&cjk, layout, &system).text, cjk);
		}

		let layout = card::Layout::new(1.).on_display(2.);
		let excerpt = information_rows::message_excerpt(
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
		let system = gpui::WindowTextSystem::new(cx.update(|cx| cx.text_system().clone()));

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
				assert_eq!(information_rows::message_excerpt(source, layout, &system).text, source);
			}
		}
	}

	#[gpui::test]
	fn short_bubbles_fit_their_text_and_long_replies_use_the_reading_column(
		cx: &mut gpui::TestAppContext,
	) {
		let visual = cx.add_empty_window();

		visual.update(|window, _| {
			let system = window.text_system();

			for density in [1., 2.] {
				for zoom in [0.51, 0.75, 1.] {
					let layout = card::Layout::new(zoom).on_display(density);
					let short = information_rows::message_excerpt("Done", layout, system);
					let width = information_rows::bubble_width(&short, layout, window.text_system());

					assert!(width < layout.details_width() / 2.);
					assert_eq!(
						information_rows::excerpt(
							short.text,
							layout,
							width - 2. * layout.bubble_padding() - layout.title_size() - 5.,
							system
						)
						.lines,
						1
					);

					let long = information_rows::message_excerpt(
						"Verify the entire recovered response, including the final paragraph, all source links, and every code block before returning the result.",
						layout,
						system,
					);

					assert!(long.lines > 1);
					assert_eq!(information_rows::bubble_width(&long, layout, window.text_system()), layout.details_width());
				}
			}
		});
	}
}
