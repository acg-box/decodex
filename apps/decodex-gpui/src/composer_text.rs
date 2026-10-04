//! Wrapped text geometry shared by painting, selection, and native input methods.
use core::panic::Location;
use std::mem;

use cursor::{Preference, Shape};
use gpui::{AvailableSpace, TextAlign};
use ui_theme::{BODY_LINE_HEIGHT, BODY_SIZE};

use crate::composer_input::{
	self, App, Bounds, ComposerAppearance, ComposerInput, Element, ElementId, ElementInputHandler,
	Entity, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Point,
	SharedString, Style, TextRun, UnderlineStyle, Window, WrappedLine, cursor, ui_theme,
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
	type RequestLayoutState = ();

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
		_: &mut App,
	) -> (LayoutId, ()) {
		let mut style = Style::default();

		style.size.width = composer_input::relative(1.0).into();

		let input = self.input.clone();
		let text_style = window.text_style();

		(
			window.request_measured_layout(style, move |known, available, window, cx| {
				let width = known.width.unwrap_or_else(|| match available.width {
					AvailableSpace::Definite(width) => width,
					_ => gpui::px(500.0),
				});
				let input = input.read(cx);
				let lines = shape_with_style(input, width, window, &text_style);
				let limit =
					if input.appearance == ComposerAppearance::Workbench { 7.0 } else { 1.0 };

				gpui::size(
					width,
					height(&lines)
						.clamp(gpui::px(BODY_LINE_HEIGHT), gpui::px(BODY_LINE_HEIGHT * limit)),
				)
			}),
			(),
		)
	}

	fn prepaint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_: &mut (),
		window: &mut Window,
		cx: &mut App,
	) -> TextPaint {
		let input = self.input.read(cx);
		let lines = shape(input, bounds.size.width, window);
		let caret = position_at(&lines, input.cursor_offset()).y;
		let max = (height(&lines) - bounds.size.height).max(gpui::px(0.0));
		let offset = if input.scroll_manually {
			input.text_offset.clamp(gpui::px(0.0), max)
		} else {
			input
				.text_offset
				.max(caret + gpui::px(BODY_LINE_HEIGHT) - bounds.size.height)
				.min(caret)
				.clamp(gpui::px(0.0), max)
		};

		TextPaint { lines, offset }
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_: &mut (),
		state: &mut TextPaint,
		window: &mut Window,
		cx: &mut App,
	) {
		let input = self.input.read(cx);
		let focus = input.focus_handle.clone();
		let selections = [input.selected_range.clone()];
		let draw_cursor = input.cursor.visible
			&& cursor::eligible(
				focus.is_focused(window),
				window.is_window_active(),
				input.selected_range.is_empty(),
			);
		let cursor_shape = Preference::configured().shape;
		let next = composer_input::next_boundary(&input.content, input.cursor_offset());
		let gutter = gpui::px(0.);
		let origin = bounds.origin + gpui::point(gutter, -state.offset);

		window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);

		let mut y = gpui::px(0.0);
		let mut start = 0;

		for line in &state.lines {
			let line_height = gpui::px(BODY_LINE_HEIGHT);
			let rows = line.wrap_boundaries().len() + 1;

			for row in 0..rows {
				let top = line_height * row;
				let first = line
					.closest_index_for_position(gpui::point(gpui::px(0.0), top), line_height)
					.unwrap_or_else(|i| i);
				let last = line
					.closest_index_for_position(
						gpui::point(bounds.size.width - gutter, top),
						line_height,
					)
					.unwrap_or_else(|i| i);

				for selection in &selections {
					let from = selection.start.max(start + first);
					let to = selection.end.min(start + last);

					if from < to {
						let x = line
							.position_for_index(from - start, line_height)
							.unwrap_or_default()
							.x;
						let end = if to == start + last {
							line.width()
						} else {
							line.position_for_index(to - start, line_height).unwrap_or_default().x
						};

						window.paint_quad(composer_input::fill(
							Bounds::new(
								origin + gpui::point(x, y + top),
								gpui::size((end - x).max(gpui::px(1.0)), line_height),
							),
							gpui::rgba(0x60a5fa40),
						));
					}
				}
			}

			let _ = line.paint(
				origin + gpui::point(gpui::px(0.0), y),
				line_height,
				TextAlign::Left,
				None,
				window,
				cx,
			);

			y += line.size(line_height).height;
			start += line.len() + 1;
		}

		if draw_cursor {
			let caret = position_at(&state.lines, selections[0].end);
			let next = position_at(&state.lines, next);
			let width = if next.y == caret.y && next.x > caret.x {
				next.x - caret.x
			} else {
				gpui::px(BODY_SIZE * 0.6)
			};
			let (offset, extent, color) = match cursor_shape {
				Shape::Bar => (
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(1.5), gpui::px(BODY_LINE_HEIGHT)),
					gpui::rgba(0xe5e7ebff),
				),
				Shape::Block => (
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(width, gpui::px(BODY_LINE_HEIGHT)),
					gpui::rgba(0xe5e7eb55),
				),
				Shape::Underline => (
					gpui::point(gpui::px(0.), gpui::px(BODY_LINE_HEIGHT - 2.)),
					gpui::size(width, gpui::px(2.)),
					gpui::rgba(0xe5e7ebff),
				),
			};

			window.paint_quad(composer_input::fill(
				Bounds::new(origin + caret + offset, extent),
				color,
			));
		}

		self.input.update(cx, |input, _| {
			input.last_layout = Some(mem::take(&mut state.lines));
			input.last_bounds = Some(Bounds::new(
				bounds.origin + gpui::point(gutter, gpui::px(0.0)),
				gpui::size(bounds.size.width - gutter, bounds.size.height),
			));
			input.text_offset = state.offset;
		});
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

fn shape(input: &ComposerInput, width: Pixels, window: &Window) -> Vec<WrappedLine> {
	let style = window.text_style();
	shape_with_style(input, width, window, &style)
}

fn shape_with_style(
	input: &ComposerInput,
	width: Pixels,
	window: &Window,
	style: &gpui::TextStyle,
) -> Vec<WrappedLine> {
	let empty = input.content.is_empty();
	let text: SharedString = if empty {
		input.placeholder.clone()
	} else if input.secret {
		"*".repeat(input.content.len()).into()
	} else {
		input.visible_content().to_owned().into()
	};
	let run = TextRun {
		len: text.len(),
		font: style.font(),
		color: if empty { gpui::rgb(0x89909c).into() } else { style.color },
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
			if empty { None } else { Some(width.max(gpui::px(1.0))) },
			None,
		)
		.unwrap_or_default()
		.into_vec()
}
