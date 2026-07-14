use std::ops::Range;

use gpui::{
	App, Bounds, Context, Element, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
	Focusable, GlobalElementId, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels,
	Point, Render, Role, ShapedLine, SharedString, Style, TextRun, UTF16Selection, Window, actions,
	div, fill, point, prelude::*, px, relative, rgb, size,
};
use unicode_segmentation::UnicodeSegmentation;

// API pattern derived from the Apache-2.0 GPUI input examples at the pinned Zed revision.
// This spike keeps only the IME/focus/cursor slice needed to prove Decodex feasibility.

actions!(decodex_gpui_spike, [Backspace, Delete, Left, Right, Home, End]);

pub fn bind_keys(cx: &mut App) {
	cx.bind_keys([
		gpui::KeyBinding::new("backspace", Backspace, Some("TextInput")),
		gpui::KeyBinding::new("delete", Delete, Some("TextInput")),
		gpui::KeyBinding::new("left", Left, Some("TextInput")),
		gpui::KeyBinding::new("right", Right, Some("TextInput")),
		gpui::KeyBinding::new("home", Home, Some("TextInput")),
		gpui::KeyBinding::new("end", End, Some("TextInput")),
	]);
}

pub struct TextInput {
	focus_handle: FocusHandle,
	content: String,
	cursor: usize,
}

impl TextInput {
	pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
		Self {
			focus_handle: cx.focus_handle().tab_index(0).tab_stop(true),
			content: String::new(),
			cursor: 0,
		}
	}

	pub fn clear(&mut self, cx: &mut Context<Self>) {
		self.content.clear();
		self.cursor = 0;
		cx.notify();
	}

	fn set_accessible_value(
		&mut self,
		data: Option<&gpui::accesskit::ActionData>,
		cx: &mut Context<Self>,
	) {
		let Some(gpui::accesskit::ActionData::Value(value)) = data else {
			return;
		};
		self.content = value.to_string();
		self.cursor = self.content.len();
		cx.notify();
	}

	pub fn content(&self) -> &str {
		&self.content
	}

	fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
		self.cursor = previous_boundary(&self.content, self.cursor);
		cx.notify();
	}

	fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
		self.cursor = next_boundary(&self.content, self.cursor);
		cx.notify();
	}

	fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
		self.cursor = 0;
		cx.notify();
	}

	fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
		self.cursor = self.content.len();
		cx.notify();
	}

	fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
		let previous = previous_boundary(&self.content, self.cursor);
		self.content.drain(previous..self.cursor);
		self.cursor = previous;
		cx.notify();
	}

	fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
		let next = next_boundary(&self.content, self.cursor);
		self.content.drain(self.cursor..next);
		cx.notify();
	}
}

impl Focusable for TextInput {
	fn focus_handle(&self, _cx: &App) -> FocusHandle {
		self.focus_handle.clone()
	}
}

impl EntityInputHandler for TextInput {
	fn text_for_range(
		&mut self,
		range_utf16: Range<usize>,
		actual_range: &mut Option<Range<usize>>,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<String> {
		let range = range_from_utf16(&self.content, &range_utf16);
		actual_range.replace(range_to_utf16(&self.content, &range));
		Some(self.content[range].to_owned())
	}

	fn selected_text_range(
		&mut self,
		_ignore_disabled_input: bool,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<UTF16Selection> {
		let cursor = offset_to_utf16(&self.content, self.cursor);
		Some(UTF16Selection { range: cursor..cursor, reversed: false })
	}

	fn marked_text_range(
		&self,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<Range<usize>> {
		None
	}

	fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

	fn replace_text_in_range(
		&mut self,
		range_utf16: Option<Range<usize>>,
		new_text: &str,
		_window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let range = range_utf16
			.as_ref()
			.map(|range| range_from_utf16(&self.content, range))
			.unwrap_or(self.cursor..self.cursor);
		self.content.replace_range(range.clone(), new_text);
		self.cursor = range.start + new_text.len();
		cx.notify();
	}

	fn replace_and_mark_text_in_range(
		&mut self,
		range_utf16: Option<Range<usize>>,
		new_text: &str,
		_new_selected_range_utf16: Option<Range<usize>>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.replace_text_in_range(range_utf16, new_text, window, cx);
	}

	fn bounds_for_range(
		&mut self,
		_range_utf16: Range<usize>,
		bounds: Bounds<Pixels>,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<Bounds<Pixels>> {
		Some(bounds)
	}

	fn character_index_for_point(
		&mut self,
		_point: Point<Pixels>,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<usize> {
		Some(offset_to_utf16(&self.content, self.cursor))
	}
}

impl Render for TextInput {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let focus_handle = self.focus_handle.clone();
		let entity = cx.entity();
		let value = SharedString::from(self.content.clone());
		div()
			.id("composer-input")
			.key_context("TextInput")
			.role(Role::TextInput)
			.aria_label("Message text input")
			.aria_placeholder("Type a message")
			.aria_value(value)
			.track_focus(&focus_handle)
			.on_a11y_action(gpui::AccessibleAction::SetValue, {
				let entity = entity.clone();
				move |data, _window, cx| {
					entity.update(cx, |input, cx| input.set_accessible_value(data, cx));
				}
			})
			.on_mouse_down(
				gpui::MouseButton::Left,
				move |_: &gpui::MouseDownEvent, window: &mut Window, cx: &mut App| {
					window.focus(&focus_handle, cx);
				},
			)
			.on_action({
				let entity = entity.clone();
				move |action: &Backspace, window, cx| {
					entity.update(cx, |input, cx| input.backspace(action, window, cx));
				}
			})
			.on_action({
				let entity = entity.clone();
				move |action: &Delete, window, cx| {
					entity.update(cx, |input, cx| input.delete(action, window, cx));
				}
			})
			.on_action({
				let entity = entity.clone();
				move |action: &Left, window, cx| {
					entity.update(cx, |input, cx| input.left(action, window, cx));
				}
			})
			.on_action({
				let entity = entity.clone();
				move |action: &Right, window, cx| {
					entity.update(cx, |input, cx| input.right(action, window, cx));
				}
			})
			.on_action({
				let entity = entity.clone();
				move |action: &Home, window, cx| {
					entity.update(cx, |input, cx| input.home(action, window, cx));
				}
			})
			.on_action(move |action: &End, window, cx| {
				entity.update(cx, |input, cx| input.end(action, window, cx));
			})
			.size_full()
			.px_3()
			.flex()
			.items_center()
			.rounded_md()
			.border_1()
			.border_color(rgb(0x4b5563))
			.bg(rgb(0x1f2937))
			.child(InputText { input: cx.entity() })
	}
}

struct InputText {
	input: Entity<TextInput>,
}

struct InputPrepaint {
	line: ShapedLine,
	cursor: Option<PaintQuad>,
}

impl IntoElement for InputText {
	type Element = Self;

	fn into_element(self) -> Self::Element {
		self
	}
}

impl Element for InputText {
	type RequestLayoutState = ();
	type PrepaintState = InputPrepaint;

	fn id(&self) -> Option<gpui::ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&InspectorElementId>,
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
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_request_layout: &mut Self::RequestLayoutState,
		window: &mut Window,
		cx: &mut App,
	) -> Self::PrepaintState {
		let input = self.input.read(cx);
		let (text, color) = if input.content.is_empty() {
			(SharedString::from("Type a message"), rgb(0x9ca3af).into())
		} else {
			(SharedString::from(input.content.clone()), rgb(0xf9fafb).into())
		};
		let style = window.text_style();
		let run = TextRun {
			len: text.len(),
			font: style.font(),
			color,
			background_color: None,
			underline: None,
			strikethrough: None,
		};
		let line = window.text_system().shape_line(
			text,
			style.font_size.to_pixels(window.rem_size()),
			&[run],
			None,
		);
		let cursor = input.focus_handle.is_focused(window).then(|| {
			fill(
				Bounds::new(
					point(bounds.left() + line.x_for_index(input.cursor), bounds.top()),
					size(px(1.5), window.line_height()),
				),
				rgb(0xf9fafb),
			)
		});
		InputPrepaint { line, cursor }
	}

	fn paint(
		&mut self,
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_request_layout: &mut Self::RequestLayoutState,
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
		prepaint
			.line
			.paint(bounds.origin, window.line_height(), gpui::TextAlign::Left, None, window, cx)
			.expect("paint input text");
		if let Some(cursor) = prepaint.cursor.take() {
			window.paint_quad(cursor);
		}
	}
}

fn previous_boundary(content: &str, offset: usize) -> usize {
	content
		.grapheme_indices(true)
		.rev()
		.find_map(|(index, _)| (index < offset).then_some(index))
		.unwrap_or(0)
}

fn next_boundary(content: &str, offset: usize) -> usize {
	content
		.grapheme_indices(true)
		.find_map(|(index, _)| (index > offset).then_some(index))
		.unwrap_or(content.len())
}

fn offset_from_utf16(content: &str, offset: usize) -> usize {
	content
		.chars()
		.scan((0usize, 0usize), |(utf8, utf16), character| {
			let current = (*utf8, *utf16);
			*utf8 += character.len_utf8();
			*utf16 += character.len_utf16();
			Some(current)
		})
		.find_map(|(utf8, utf16)| (utf16 >= offset).then_some(utf8))
		.unwrap_or(content.len())
}

fn offset_to_utf16(content: &str, offset: usize) -> usize {
	content[..offset].encode_utf16().count()
}

fn range_from_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
	offset_from_utf16(content, range.start)..offset_from_utf16(content, range.end)
}

fn range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
	offset_to_utf16(content, range.start)..offset_to_utf16(content, range.end)
}

#[cfg(test)]
mod tests {
	use gpui::{TestAppContext, VisualTestContext};

	use super::*;

	#[gpui::test]
	fn text_input_focus_typing_and_keyboard_are_deterministic(cx: &mut TestAppContext) {
		cx.update(bind_keys);
		let window = cx.update(|cx| {
			cx.open_window(Default::default(), |window, cx| cx.new(|cx| TextInput::new(window, cx)))
				.expect("open test window")
		});
		let mut visual = VisualTestContext::from_window(window.into(), cx);
		let input = window.root(&mut visual).expect("root input");
		visual.update(|window, cx| {
			let focus = input.read(cx).focus_handle.clone();
			window.focus(&focus, cx);
		});

		visual.simulate_input("hello 日本");
		visual.simulate_keystrokes("left backspace end");

		assert_eq!(input.read_with(&visual, |input, _| input.content.clone()), "hello 本");
		let focused = visual.update(|window, cx| input.read(cx).focus_handle.is_focused(window));
		assert!(focused);
	}
}
