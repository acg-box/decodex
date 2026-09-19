//! Wrapped text geometry shared by painting, selection, and native input methods.
use super::*;

pub(super) struct ComposerTextElement {
	pub(super) input: Entity<ComposerInput>,
}

pub(super) struct TextPaint {
	lines: Vec<WrappedLine>,
	offset: Pixels,
}

fn height(lines: &[WrappedLine]) -> Pixels {
	lines.iter().map(|line| line.size(px(ui_theme::BODY_LINE_HEIGHT)).height).sum()
}

pub(super) fn position_at(lines: &[WrappedLine], index: usize) -> Point<Pixels> {
	let mut start = 0;
	let mut y = px(0.0);
	for line in lines {
		if index <= start + line.len() {
			return line
				.position_for_index(index - start, px(ui_theme::BODY_LINE_HEIGHT))
				.unwrap_or_default()
				+ point(px(0.0), y);
		}
		start += line.len() + 1;
		y += line.size(px(ui_theme::BODY_LINE_HEIGHT)).height;
	}
	point(px(0.0), y)
}

pub(super) fn index_at(lines: &[WrappedLine], position: Point<Pixels>) -> usize {
	let mut start = 0;
	let mut y = px(0.0);
	for line in lines {
		let next = y + line.size(px(ui_theme::BODY_LINE_HEIGHT)).height;
		if position.y < next {
			return start
				+ line
					.closest_index_for_position(
						point(position.x, (position.y - y).max(px(0.0))),
						px(ui_theme::BODY_LINE_HEIGHT),
					)
					.unwrap_or_else(|index| index);
		}
		start += line.len() + 1;
		y = next;
	}
	start.saturating_sub(1)
}

fn shape(input: &ComposerInput, width: Pixels, window: &Window) -> Vec<WrappedLine> {
	let empty = input.content.is_empty();
	let text: SharedString = if empty {
		input.placeholder.clone()
	} else if input.secret {
		"*".repeat(input.content.len()).into()
	} else {
		input.content.clone().into()
	};
	let style = window.text_style();
	let run = TextRun {
		len: text.len(),
		font: style.font(),
		color: if empty { rgb(0x89909c).into() } else { style.color },
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
					thickness: px(1.0),
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
			if empty { None } else { Some(width.max(px(1.0))) },
			None,
		)
		.unwrap_or_default()
		.into_vec()
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

	fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
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
		style.size.width = relative(1.0).into();
		let input = self.input.clone();
		(
			window.request_measured_layout(style, move |known, available, window, cx| {
				let width = known.width.unwrap_or_else(|| match available.width {
					gpui::AvailableSpace::Definite(width) => width,
					_ => px(500.0),
				});
				let input = input.read(cx);
				let lines = shape(input, width, window);
				let limit =
					if input.appearance == ComposerAppearance::Workbench { 7.0 } else { 1.0 };
				size(
					width,
					height(&lines).clamp(
						px(ui_theme::BODY_LINE_HEIGHT),
						px(ui_theme::BODY_LINE_HEIGHT * limit),
					),
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
		let max = (height(&lines) - bounds.size.height).max(px(0.0));
		let offset = if input.scroll_manually {
			input.text_offset.clamp(px(0.0), max)
		} else {
			input
				.text_offset
				.max(caret + px(ui_theme::BODY_LINE_HEIGHT) - bounds.size.height)
				.min(caret)
				.clamp(px(0.0), max)
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
		let gutter = px(0.);
		let origin = bounds.origin + point(gutter, -state.offset);
		window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);
		let mut y = px(0.0);
		let mut start = 0;
		for line in &state.lines {
			let line_height = px(ui_theme::BODY_LINE_HEIGHT);
			let rows = line.wrap_boundaries().len() + 1;
			for row in 0..rows {
				let top = line_height * row;
				let first = line
					.closest_index_for_position(point(px(0.0), top), line_height)
					.unwrap_or_else(|i| i);
				let last = line
					.closest_index_for_position(point(bounds.size.width - gutter, top), line_height)
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
						window.paint_quad(fill(
							Bounds::new(
								origin + point(x, y + top),
								size((end - x).max(px(1.0)), line_height),
							),
							rgba(0x60a5fa40),
						));
					}
				}
			}
			let _ = line.paint(
				origin + point(px(0.0), y),
				line_height,
				gpui::TextAlign::Left,
				None,
				window,
				cx,
			);
			y += line.size(line_height).height;
			start += line.len() + 1;
		}
		if focus.is_focused(window) {
			for selection in &selections {
				let caret = position_at(&state.lines, selection.end);
				window.paint_quad(fill(
					Bounds::new(origin + caret, size(px(1.5), px(ui_theme::BODY_LINE_HEIGHT))),
					rgb(0xe5e7eb),
				));
			}
		}

		self.input.update(cx, |input, _| {
			input.last_layout = Some(std::mem::take(&mut state.lines));
			input.last_bounds = Some(Bounds::new(
				bounds.origin + point(gutter, px(0.0)),
				size(bounds.size.width - gutter, bounds.size.height),
			));
			input.text_offset = state.offset;
		});
	}
}
