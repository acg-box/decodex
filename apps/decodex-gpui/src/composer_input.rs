//! Native bounded text input for the Conversation composer.

use std::ops::Range;

use gpui::{
	AccessibleAction, App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId,
	ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
	GlobalElementId, InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton,
	MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render, Role,
	ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div,
	fill, point, prelude::*, px, relative, rgb, rgba, size,
};

use crate::ui_theme;

pub(crate) const MAX_COMPOSER_BYTES: usize = 16 * 1_024;

actions!(
	decodex_composer_input,
	[
		Backspace,
		Delete,
		Left,
		Right,
		SelectLeft,
		SelectRight,
		SelectAll,
		Home,
		End,
		InsertNewline,
		ShowCharacterPalette,
		Paste,
		Cut,
		Copy,
		SubmitComposer,
	]
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComposerEvent {
	Changed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComposerAppearance {
	Workbench,
	Field,
}

impl EventEmitter<ComposerEvent> for ComposerInput {}

pub(crate) fn bind_keys(cx: &mut App) {
	cx.bind_keys([
		KeyBinding::new("backspace", Backspace, Some("ComposerInput")),
		KeyBinding::new("delete", Delete, Some("ComposerInput")),
		KeyBinding::new("left", Left, Some("ComposerInput")),
		KeyBinding::new("right", Right, Some("ComposerInput")),
		KeyBinding::new("shift-left", SelectLeft, Some("ComposerInput")),
		KeyBinding::new("shift-right", SelectRight, Some("ComposerInput")),
		KeyBinding::new("cmd-a", SelectAll, Some("ComposerInput")),
		KeyBinding::new("home", Home, Some("ComposerInput")),
		KeyBinding::new("end", End, Some("ComposerInput")),
		KeyBinding::new("shift-enter", InsertNewline, Some("ComposerInput")),
		KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some("ComposerInput")),
		KeyBinding::new("cmd-v", Paste, Some("ComposerInput")),
		KeyBinding::new("cmd-x", Cut, Some("ComposerInput")),
		KeyBinding::new("cmd-c", Copy, Some("ComposerInput")),
		KeyBinding::new("enter", SubmitComposer, Some("ComposerInput")),
	]);
}

/// Conversation composer input and its native text-input lifecycle.
pub(crate) struct ComposerInput {
	focus_handle: FocusHandle,
	placeholder: SharedString,
	aria_label: SharedString,
	content: String,
	selected_range: Range<usize>,
	selection_reversed: bool,
	marked_range: Option<Range<usize>>,
	last_layout: Option<ShapedLine>,
	last_bounds: Option<Bounds<Pixels>>,
	is_selecting: bool,
	appearance: ComposerAppearance,
}

impl ComposerInput {
	pub(crate) fn new(tab_index: isize, cx: &mut Context<Self>) -> Self {
		Self::build(
			tab_index,
			"Message Codex…",
			"Conversation message",
			ComposerAppearance::Workbench,
			cx,
		)
	}

	pub(crate) fn with_placeholder(
		tab_index: isize,
		placeholder: impl Into<SharedString>,
		aria_label: impl Into<SharedString>,
		cx: &mut Context<Self>,
	) -> Self {
		Self::build(tab_index, placeholder, aria_label, ComposerAppearance::Field, cx)
	}

	fn build(
		tab_index: isize,
		placeholder: impl Into<SharedString>,
		aria_label: impl Into<SharedString>,
		appearance: ComposerAppearance,
		cx: &mut Context<Self>,
	) -> Self {
		Self {
			focus_handle: cx.focus_handle().tab_index(tab_index).tab_stop(true),
			placeholder: placeholder.into(),
			aria_label: aria_label.into(),
			content: String::new(),
			selected_range: 0..0,
			selection_reversed: false,
			marked_range: None,
			last_layout: None,
			last_bounds: None,
			is_selecting: false,
			appearance,
		}
	}

	pub(crate) fn content(&self) -> &str {
		&self.content
	}

	pub(crate) fn len(&self) -> usize {
		self.content.len()
	}

	pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
		if self.content.is_empty() {
			return;
		}
		self.content.clear();
		self.selected_range = 0..0;
		self.selection_reversed = false;
		self.marked_range = None;
		self.last_layout = None;
		self.changed(cx);
	}

	pub(crate) fn set_content(&mut self, value: &str, cx: &mut Context<Self>) {
		if self.content == value {
			return;
		}
		self.replace_bytes(0..self.content.len(), value, false, None, cx);
	}

	fn changed(&mut self, cx: &mut Context<Self>) {
		cx.notify();
		cx.emit(ComposerEvent::Changed);
	}

	fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			self.move_to(previous_boundary(&self.content, self.cursor_offset()), cx);
		} else {
			self.move_to(self.selected_range.start, cx);
		}
	}

	fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			self.move_to(next_boundary(&self.content, self.cursor_offset()), cx);
		} else {
			self.move_to(self.selected_range.end, cx);
		}
	}

	fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
		self.select_to(previous_boundary(&self.content, self.cursor_offset()), cx);
	}

	fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
		self.select_to(next_boundary(&self.content, self.cursor_offset()), cx);
	}

	fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
		self.selected_range = 0..self.content.len();
		self.selection_reversed = false;
		self.marked_range = None;
		cx.notify();
	}

	fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
		self.move_to(0, cx);
	}

	fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
		self.move_to(self.content.len(), cx);
	}

	fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			let previous = previous_boundary(&self.content, self.cursor_offset());
			if previous == self.cursor_offset() {
				window.play_system_bell();
				return;
			}
			self.select_to(previous, cx);
		}
		self.replace_text_in_range(None, "", window, cx);
	}

	fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			let next = next_boundary(&self.content, self.cursor_offset());
			if next == self.cursor_offset() {
				window.play_system_bell();
				return;
			}
			self.select_to(next, cx);
		}
		self.replace_text_in_range(None, "", window, cx);
	}

	fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
		self.replace_text_in_range(None, "\n", window, cx);
	}

	fn show_character_palette(
		&mut self,
		_: &ShowCharacterPalette,
		window: &mut Window,
		_: &mut Context<Self>,
	) {
		window.show_character_palette();
	}

	fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
			self.replace_text_in_range(None, &text, window, cx);
		}
	}

	fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
		if !self.selected_range.is_empty() {
			cx.write_to_clipboard(ClipboardItem::new_string(
				self.content[self.selected_range.clone()].to_owned(),
			));
		}
	}

	fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			return;
		}
		cx.write_to_clipboard(ClipboardItem::new_string(
			self.content[self.selected_range.clone()].to_owned(),
		));
		self.replace_text_in_range(None, "", window, cx);
	}

	fn on_mouse_down(
		&mut self,
		event: &MouseDownEvent,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		window.focus(&self.focus_handle, cx);
		self.is_selecting = true;
		let offset = self.index_for_mouse_position(event.position);
		if event.modifiers.shift {
			self.select_to(offset, cx);
		} else {
			self.move_to(offset, cx);
		}
	}

	fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
		self.is_selecting = false;
	}

	fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
		if self.is_selecting {
			self.select_to(self.index_for_mouse_position(event.position), cx);
		}
	}

	fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
		let offset = offset.min(self.content.len());
		self.selected_range = offset..offset;
		self.selection_reversed = false;
		self.marked_range = None;
		cx.notify();
	}

	fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
		let offset = offset.min(self.content.len());
		let anchor = if self.selection_reversed {
			self.selected_range.end
		} else {
			self.selected_range.start
		};
		if offset < anchor {
			self.selected_range = offset..anchor;
			self.selection_reversed = true;
		} else {
			self.selected_range = anchor..offset;
			self.selection_reversed = false;
		}
		self.marked_range = None;
		cx.notify();
	}

	fn cursor_offset(&self) -> usize {
		if self.selection_reversed { self.selected_range.start } else { self.selected_range.end }
	}

	fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
		if self.content.is_empty() {
			return 0;
		}
		let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
		else {
			return self.cursor_offset();
		};
		if position.y < bounds.top() {
			return 0;
		}
		if position.y > bounds.bottom() {
			return self.content.len();
		}
		line.closest_index_for_x(position.x - bounds.left())
	}

	fn replacement_range(&self, range_utf16: Option<&Range<usize>>) -> Range<usize> {
		range_utf16
			.map(|range| range_from_utf16(&self.content, range))
			.or_else(|| self.marked_range.clone())
			.unwrap_or_else(|| self.selected_range.clone())
	}

	fn replace_bytes(
		&mut self,
		range: Range<usize>,
		new_text: &str,
		mark: bool,
		selected_range_utf16: Option<&Range<usize>>,
		cx: &mut Context<Self>,
	) {
		let retained = self.content.len().saturating_sub(range.end.saturating_sub(range.start));
		let replacement = bounded_input(new_text, MAX_COMPOSER_BYTES.saturating_sub(retained));
		let inserted = range.start..range.start + replacement.len();
		let relative_selection =
			selected_range_utf16.map(|selection| range_from_utf16(&replacement, selection));
		self.content.replace_range(range, &replacement);
		self.marked_range = mark.then(|| inserted.clone()).filter(|range| !range.is_empty());
		self.selected_range = relative_selection.map_or_else(
			|| inserted.end..inserted.end,
			|selection| inserted.start + selection.start..inserted.start + selection.end,
		);
		self.selection_reversed = false;
		self.last_layout = None;
		self.changed(cx);
	}

	fn set_accessible_value(
		&mut self,
		data: Option<&gpui::accesskit::ActionData>,
		cx: &mut Context<Self>,
	) {
		let Some(gpui::accesskit::ActionData::Value(value)) = data else {
			return;
		};
		self.replace_bytes(0..self.content.len(), value, false, None, cx);
	}
}

impl Focusable for ComposerInput {
	fn focus_handle(&self, _: &App) -> FocusHandle {
		self.focus_handle.clone()
	}
}

impl EntityInputHandler for ComposerInput {
	fn text_for_range(
		&mut self,
		range_utf16: Range<usize>,
		actual_range: &mut Option<Range<usize>>,
		_: &mut Window,
		_: &mut Context<Self>,
	) -> Option<String> {
		let range = range_from_utf16(&self.content, &range_utf16);
		actual_range.replace(range_to_utf16(&self.content, &range));
		Some(self.content[range].to_owned())
	}

	fn selected_text_range(
		&mut self,
		_: bool,
		_: &mut Window,
		_: &mut Context<Self>,
	) -> Option<UTF16Selection> {
		Some(UTF16Selection {
			range: range_to_utf16(&self.content, &self.selected_range),
			reversed: self.selection_reversed,
		})
	}

	fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
		self.marked_range.as_ref().map(|range| range_to_utf16(&self.content, range))
	}

	fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		self.marked_range = None;
		cx.notify();
	}

	fn replace_text_in_range(
		&mut self,
		range_utf16: Option<Range<usize>>,
		new_text: &str,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		let range = self.replacement_range(range_utf16.as_ref());
		self.replace_bytes(range, new_text, false, None, cx);
	}

	fn replace_and_mark_text_in_range(
		&mut self,
		range_utf16: Option<Range<usize>>,
		new_text: &str,
		new_selected_range_utf16: Option<Range<usize>>,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		let range = self.replacement_range(range_utf16.as_ref());
		self.replace_bytes(range, new_text, true, new_selected_range_utf16.as_ref(), cx);
	}

	fn bounds_for_range(
		&mut self,
		range_utf16: Range<usize>,
		bounds: Bounds<Pixels>,
		_: &mut Window,
		_: &mut Context<Self>,
	) -> Option<Bounds<Pixels>> {
		let line = self.last_layout.as_ref()?;
		let range = range_from_utf16(&self.content, &range_utf16);
		Some(Bounds::from_corners(
			point(bounds.left() + line.x_for_index(range.start), bounds.top()),
			point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
		))
	}

	fn character_index_for_point(
		&mut self,
		point: Point<Pixels>,
		_: &mut Window,
		_: &mut Context<Self>,
	) -> Option<usize> {
		Some(offset_to_utf16(&self.content, self.index_for_mouse_position(point)))
	}

	fn set_selected_text_range(
		&mut self,
		range_utf16: Range<usize>,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.selected_range = range_from_utf16(&self.content, &range_utf16);
		self.selection_reversed = false;
		self.marked_range = None;
		cx.notify();
	}

	fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
		Some(offset_to_utf16(&self.content, self.content.len()))
	}
}

impl Render for ComposerInput {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let entity = cx.entity();
		let focus_handle = self.focus_handle.clone();
		let workbench = self.appearance == ComposerAppearance::Workbench;
		div()
			.id("conversation-composer-input")
			.key_context("ComposerInput")
			.role(Role::TextInput)
			.aria_label(self.aria_label.clone())
			.aria_placeholder(self.placeholder.clone())
			.aria_value(self.content.clone())
			.track_focus(&focus_handle)
			.on_a11y_action(AccessibleAction::SetValue, {
				let entity = entity.clone();
				move |data, _, cx| {
					entity.update(cx, |input, cx| input.set_accessible_value(data, cx));
				}
			})
			.on_action(cx.listener(Self::backspace))
			.on_action(cx.listener(Self::delete))
			.on_action(cx.listener(Self::left))
			.on_action(cx.listener(Self::right))
			.on_action(cx.listener(Self::select_left))
			.on_action(cx.listener(Self::select_right))
			.on_action(cx.listener(Self::select_all))
			.on_action(cx.listener(Self::home))
			.on_action(cx.listener(Self::end))
			.on_action(cx.listener(Self::insert_newline))
			.on_action(cx.listener(Self::show_character_palette))
			.on_action(cx.listener(Self::paste))
			.on_action(cx.listener(Self::cut))
			.on_action(cx.listener(Self::copy))
			.on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
			.on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
			.on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
			.on_mouse_move(cx.listener(Self::on_mouse_move))
			.cursor(CursorStyle::IBeam)
			.size_full()
			.px_2()
			.py_2()
			.flex()
			.items_center()
			.overflow_hidden()
			.rounded(px(if workbench { 8.0 } else { 6.0 }))
			.border_1()
			.border_color(if workbench {
				rgba(0x00000000)
			} else if focus_handle.is_focused(window) {
				rgb(0x817789)
			} else {
				rgb(0x3c3744)
			})
			.bg(if workbench { rgba(0x00000000) } else { rgba(ui_theme::FIELD_MATERIAL) })
			.text_size(px(if workbench { 10.5 } else { 11.0 }))
			.text_color(rgb(0xeeeaf0))
			.child(ComposerTextElement { input: entity })
	}
}

struct ComposerTextElement {
	input: Entity<ComposerInput>,
}

struct ComposerTextPrepaint {
	line: Option<ShapedLine>,
	cursor: Option<PaintQuad>,
	selection: Option<PaintQuad>,
}

impl IntoElement for ComposerTextElement {
	type Element = Self;

	fn into_element(self) -> Self::Element {
		self
	}
}

impl Element for ComposerTextElement {
	type PrepaintState = ComposerTextPrepaint;
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
		cx: &mut App,
	) -> (LayoutId, Self::RequestLayoutState) {
		let mut style = Style::default();
		style.size.width = relative(1.0).into();
		style.size.height = window.line_height().into();
		(window.request_layout(style, [], cx), ())
	}

	fn prepaint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_: &mut Self::RequestLayoutState,
		window: &mut Window,
		cx: &mut App,
	) -> Self::PrepaintState {
		let input = self.input.read(cx);
		let content_is_empty = input.content.is_empty();
		let display_text: SharedString = if content_is_empty {
			input.placeholder.clone()
		} else {
			input.content.replace('\n', " ").into()
		};
		let style = window.text_style();
		let run = TextRun {
			len: display_text.len(),
			font: style.font(),
			color: if content_is_empty { rgb(0x64748b).into() } else { style.color },
			background_color: None,
			underline: None,
			strikethrough: None,
		};
		let runs = if let Some(marked) = input.marked_range.as_ref() {
			vec![
				TextRun { len: marked.start, ..run.clone() },
				TextRun {
					len: marked.end - marked.start,
					underline: Some(UnderlineStyle {
						color: Some(run.color),
						thickness: px(1.0),
						wavy: false,
					}),
					..run.clone()
				},
				TextRun { len: display_text.len() - marked.end, ..run },
			]
			.into_iter()
			.filter(|run| run.len > 0)
			.collect::<Vec<_>>()
		} else {
			vec![run]
		};
		let line = window.text_system().shape_line(
			display_text,
			style.font_size.to_pixels(window.rem_size()),
			&runs,
			None,
		);
		let cursor_offset = input.cursor_offset();
		let selected_range = input.selected_range.clone();
		let focused = input.focus_handle.is_focused(window);
		let (selection, cursor) = if selected_range.is_empty() {
			(
				None,
				focused.then(|| {
					fill(
						Bounds::new(
							point(bounds.left() + line.x_for_index(cursor_offset), bounds.top()),
							size(px(1.5), bounds.bottom() - bounds.top()),
						),
						rgb(0xe5e7eb),
					)
				}),
			)
		} else {
			(
				Some(fill(
					Bounds::from_corners(
						point(bounds.left() + line.x_for_index(selected_range.start), bounds.top()),
						point(
							bounds.left() + line.x_for_index(selected_range.end),
							bounds.bottom(),
						),
					),
					rgba(0x60a5fa30),
				)),
				None,
			)
		};
		ComposerTextPrepaint { line: Some(line), cursor, selection }
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_: &mut Self::RequestLayoutState,
		prepaint: &mut Self::PrepaintState,
		window: &mut Window,
		cx: &mut App,
	) {
		let focus_handle = self.input.read(cx).focus_handle.clone();
		window.handle_input(
			&focus_handle,
			ElementInputHandler::new(bounds, self.input.clone()),
			cx,
		);
		if let Some(selection) = prepaint.selection.take() {
			window.paint_quad(selection);
		}
		let line = prepaint.line.take().expect("composer line was shaped");
		let _ = line.paint(
			bounds.origin,
			window.line_height(),
			gpui::TextAlign::Left,
			None,
			window,
			cx,
		);
		if let Some(cursor) = prepaint.cursor.take() {
			window.paint_quad(cursor);
		}
		self.input.update(cx, |input, _| {
			input.last_layout = Some(line);
			input.last_bounds = Some(bounds);
		});
	}
}

fn previous_boundary(content: &str, offset: usize) -> usize {
	content[..offset.min(content.len())].char_indices().next_back().map_or(0, |(index, _)| index)
}

fn next_boundary(content: &str, offset: usize) -> usize {
	let offset = offset.min(content.len());
	content[offset..]
		.chars()
		.next()
		.map_or(content.len(), |character| offset + character.len_utf8())
}

fn offset_from_utf16(content: &str, offset: usize) -> usize {
	let mut utf8 = 0;
	let mut utf16 = 0;
	for character in content.chars() {
		if utf16 >= offset {
			break;
		}
		utf8 += character.len_utf8();
		utf16 += character.len_utf16();
	}
	utf8
}

fn offset_to_utf16(content: &str, offset: usize) -> usize {
	let mut utf8 = 0;
	let mut utf16 = 0;
	for character in content.chars() {
		if utf8 >= offset {
			break;
		}
		utf8 += character.len_utf8();
		utf16 += character.len_utf16();
	}
	utf16
}

fn range_from_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
	let start = offset_from_utf16(content, range.start);
	let end = offset_from_utf16(content, range.end).max(start);
	start..end
}

fn range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
	offset_to_utf16(content, range.start)..offset_to_utf16(content, range.end)
}

fn bounded_input(value: &str, maximum_bytes: usize) -> String {
	let mut output = String::new();
	let mut characters = value.chars().peekable();
	while let Some(mut character) = characters.next() {
		if character == '\r' {
			if characters.peek() == Some(&'\n') {
				characters.next();
			}
			character = '\n';
		}
		if character.is_control() && character != '\n' {
			continue;
		}
		if output.len().saturating_add(character.len_utf8()) > maximum_bytes {
			break;
		}
		output.push(character);
	}
	output
}
