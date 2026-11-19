//! Wrapped text geometry shared by painting, selection, and native input methods.
use core::panic::Location;
use std::{mem, ops::Range};

use gpui::{AvailableSpace, Size, TextAlign, TextStyle};
use ui_theme::{BODY_LINE_HEIGHT, BODY_SIZE, CONTROL_GAP};

use crate::{
	composer_input::{
		self, App, Bounds, ComposerInput, Element, ElementId, ElementInputHandler, Entity,
		GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Point, SharedString,
		Style, TextRun, UnderlineStyle, Window, WrappedLine, cursor, hint::SuggestionActions,
		ui_theme,
	},
	ui_motion,
};

pub(super) struct ComposerTextElement {
	pub(super) input: Entity<ComposerInput>,
}
impl IntoElement for ComposerTextElement {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}

impl Element for ComposerTextElement {
	type PrepaintState = TextPaint;
	type RequestLayoutState = Option<SuggestionActions>;

	fn id(&self) -> Option<ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (LayoutId, Self::RequestLayoutState) {
		let mut style = Style::default();

		style.size.width = composer_input::relative(1.0).into();

		let input = self.input.clone();
		let text_style = window.text_style();
		let actions = SuggestionActions::measure(&input, window, cx);
		let action_size = actions.as_ref().map(|actions| actions.size);
		(
			window.request_measured_layout(style, move |known, available, window, cx| {
				let width = known.width.unwrap_or_else(|| match available.width {
					AvailableSpace::Definite(width) => width,
					_ => gpui::px(500.0),
				});
				let input = input.read(cx);
				let layout = layout_text(input, width, action_size, window, &text_style);
				let limit = if input.appearance.multiline() { 6.0 } else { 1.0 };
				gpui::size(
					width,
					layout
						.height
						.clamp(gpui::px(BODY_LINE_HEIGHT), gpui::px(BODY_LINE_HEIGHT * limit)),
				)
			}),
			actions,
		)
	}

	fn prepaint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		actions: &mut Self::RequestLayoutState,
		window: &mut Window,
		cx: &mut App,
	) -> TextPaint {
		let input = self.input.read(cx);
		let action_size = actions.as_ref().map(|actions| actions.size);
		let layout =
			layout_text(input, bounds.size.width, action_size, window, &window.text_style());
		let caret = position_at(&layout.lines, input.cursor_offset()).y;
		let max = (layout.height - bounds.size.height).max(gpui::px(0.0));
		let offset = if input.scroll_manually {
			input.text_offset.clamp(gpui::px(0.0), max)
		} else {
			input
				.text_offset
				.max(caret + gpui::px(BODY_LINE_HEIGHT) - bounds.size.height)
				.min(caret)
				.clamp(gpui::px(0.0), max)
		};

		if let (Some(actions), Some(position)) = (actions, layout.action_origin) {
			let origin = bounds.origin + position - gpui::point(gpui::px(0.), offset);
			actions.element.prepaint_at(origin, window, cx);
		}

		TextPaint { lines: layout.lines, offset }
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		actions: &mut Self::RequestLayoutState,
		state: &mut TextPaint,
		window: &mut Window,
		cx: &mut App,
	) {
		let input = self.input.read(cx);
		let focus = input.focus_handle.clone();
		let selection = input.selected_range.clone();
		let draw_cursor = cursor::eligible(
			focus.is_focused(window),
			window.is_window_active() && ui_motion::application_active(),
			selection.is_empty(),
		);
		let next = composer_input::next_boundary(&input.content, input.cursor_offset());
		let origin = bounds.origin - gpui::point(gpui::px(0.), state.offset);

		window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);

		let line_height = gpui::px(BODY_LINE_HEIGHT);
		let mut line_origin = origin;
		let mut start = 0;

		for line in &state.lines {
			let from = selection.start.saturating_sub(start);
			let to = selection.end.saturating_sub(start).min(line.len());
			if from < to {
				paint_selection(line, from..to, line_origin, window);
			}

			let _ = line.paint(line_origin, line_height, TextAlign::Left, None, window, cx);

			line_origin.y += line.size(line_height).height;
			start += line.len() + 1;
		}

		if let Some(actions) = actions {
			actions.element.paint(window, cx);
		}

		if draw_cursor {
			let caret = position_at(&state.lines, selection.end);
			let next = position_at(&state.lines, next);
			let width = if next.y == caret.y && next.x > caret.x {
				next.x - caret.x
			} else {
				gpui::px(BODY_SIZE * 0.6)
			};

			self.input.update(cx, |input, cx| {
				input.cursor.paint(
					selection.end,
					Bounds::new(origin + caret, gpui::size(width, line_height)),
					bounds,
					state.offset,
					window,
					cx,
				);
			});
		}

		self.input.update(cx, |input, _| {
			input.last_layout = Some(mem::take(&mut state.lines));
			input.last_bounds = Some(bounds);
			input.text_offset = state.offset;
		});
	}
}

fn paint_selection(
	line: &WrappedLine,
	selection: Range<usize>,
	origin: Point<Pixels>,
	window: &mut Window,
) {
	let line_height = gpui::px(BODY_LINE_HEIGHT);
	let color = ui_theme::rgba(0x60a5fa40);
	let paint_span = |window: &mut Window, origin: Point<Pixels>, start: Pixels, end: Pixels| {
		window.paint_quad(composer_input::fill(
			Bounds::new(
				origin + gpui::point(start, gpui::px(0.)),
				gpui::size((end - start).max(gpui::px(1.)), line_height),
			),
			color,
		));
	};
	let mut wraps = line.wrap_boundaries().iter().peekable();
	let mut row_origin = origin;
	let mut span_start = None;

	for (run_ix, run) in line.runs().iter().enumerate() {
		for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
			let x = glyph.position.x;
			let wrapped = wraps
				.next_if(|boundary| boundary.run_ix == run_ix && boundary.glyph_ix == glyph_ix)
				.is_some();
			let selected = selection.contains(&glyph.index);
			if (wrapped || !selected)
				&& let Some(start) = span_start.take()
			{
				paint_span(window, row_origin, start, x);
			}
			if wrapped {
				row_origin = gpui::point(origin.x - x, row_origin.y + line_height);
			}
			if selected {
				span_start.get_or_insert(x);
			}
		}
	}

	if let Some(start) = span_start {
		paint_span(window, row_origin, start, line.unwrapped_layout.width);
	}
}

pub(super) struct TextPaint {
	lines: Vec<WrappedLine>,
	offset: Pixels,
}

pub(super) fn position_at(lines: &[WrappedLine], index: usize) -> Point<Pixels> {
	let mut start = 0;
	let mut y = gpui::px(0.0);

	for line in lines {
		if index <= start + line.len() {
			return line
				.position_for_index(index - start, gpui::px(BODY_LINE_HEIGHT))
				.unwrap_or_default()
				+ gpui::point(gpui::px(0.0), y);
		}

		start += line.len() + 1;
		y += line.size(gpui::px(BODY_LINE_HEIGHT)).height;
	}

	gpui::point(gpui::px(0.0), y)
}

pub(super) fn index_at(lines: &[WrappedLine], position: Point<Pixels>) -> usize {
	let mut start = 0;
	let mut y = gpui::px(0.0);

	for line in lines {
		let next = y + line.size(gpui::px(BODY_LINE_HEIGHT)).height;

		if position.y < next {
			return start
				+ line
					.closest_index_for_position(
						gpui::point(position.x, (position.y - y).max(gpui::px(0.0))),
						gpui::px(BODY_LINE_HEIGHT),
					)
					.unwrap_or_else(|index| index);
		}

		start += line.len() + 1;
		y = next;
	}

	start.saturating_sub(1)
}

fn height(lines: &[WrappedLine]) -> Pixels {
	lines.iter().map(|line| line.size(gpui::px(BODY_LINE_HEIGHT)).height).sum()
}

struct TextLayout {
	lines: Vec<WrappedLine>,
	height: Pixels,
	action_origin: Option<Point<Pixels>>,
}

fn layout_text(
	input: &ComposerInput,
	width: Pixels,
	action_size: Option<Size<Pixels>>,
	window: &Window,
	style: &TextStyle,
) -> TextLayout {
	let text = if action_size.is_some() {
		input.placeholder.trim_end().into()
	} else if input.content.is_empty() {
		input.placeholder.clone()
	} else if input.secret {
		"*".repeat(input.content.len()).into()
	} else {
		input.visible_content().to_owned().into()
	};
	let mut lines = shape_text(input, text, width, window, style);
	let mut text_height = height(&lines);
	let action_origin = action_size.map(|actions| {
		let gap = gpui::px(CONTROL_GAP);
		let available = width - actions.width - gap;
		let mut end = end_of_last_row(&lines);

		if end > available
			&& let Some(last) = lines.last()
		{
			// Reflow the last visible row so the text carries the controls onto
			// the next row. Earlier rows get the full text width.
			let row_start = last_row_start(last).0;
			let tail = shape_text(input, last.text[row_start..].into(), available, window, style);
			if let Some(row) = tail.first()
				&& let Some(boundary) = row.wrap_boundaries().last()
			{
				let split = row_start + row.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index;
				let (before, after) = last.text.split_at(split);
				let text = format!("{}\n{}", before.trim_end(), after.trim_start());
				lines.pop();
				lines.extend(shape_text(input, text.into(), width, window, style));
				text_height = height(&lines);
				end = end_of_last_row(&lines);
			}
		}

		let line_height = gpui::px(BODY_LINE_HEIGHT);
		let x = if end <= available {
			end + gap
		} else {
			text_height += line_height;
			(width - actions.width).max(gpui::px(0.))
		};
		gpui::point(x, text_height - (line_height + actions.height) / 2.)
	});
	TextLayout { lines, height: text_height, action_origin }
}

fn last_row_start(line: &WrappedLine) -> (usize, Pixels) {
	line.wrap_boundaries().last().map_or((0, gpui::px(0.)), |boundary| {
		let glyph = &line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix];
		(glyph.index, glyph.position.x)
	})
}

fn end_of_last_row(lines: &[WrappedLine]) -> Pixels {
	lines.last().map_or(gpui::px(0.), |line| line.unwrapped_layout.width - last_row_start(line).1)
}

fn shape_text(
	input: &ComposerInput,
	text: SharedString,
	width: Pixels,
	window: &Window,
	style: &TextStyle,
) -> Vec<WrappedLine> {
	let empty = input.content.is_empty();
	let run = TextRun {
		len: text.len(),
		font: style.font(),
		color: if empty { crate::ui_theme::rgb(ui_theme::TEXT_MUTED).into() } else { style.color },
		background_color: None,
		underline: None,
		strikethrough: None,
	};
	let runs = if let Some(mark) = &input.marked_range {
		vec![
			TextRun { len: mark.start, ..run.clone() },
			TextRun {
				len: mark.len(),
				underline: Some(UnderlineStyle {
					color: Some(run.color),
					thickness: gpui::px(1.0),
					wavy: false,
				}),
				..run.clone()
			},
			TextRun { len: text.len().saturating_sub(mark.end), ..run },
		]
	} else {
		vec![run]
	};

	window
		.text_system()
		.shape_text(
			text,
			style.font_size.to_pixels(window.rem_size()),
			&runs,
			if input.appearance.multiline() || !empty {
				Some(width.max(gpui::px(1.0)))
			} else {
				None
			},
			None,
		)
		.unwrap_or_default()
		.into_vec()
}
