//! Native bounded text input for the Conversation composer.
#[path = "composer_cursor.rs"] pub(crate) mod cursor;

#[path = "composer_edit.rs"] mod edit;
#[path = "composer_hint.rs"] mod hint;
#[cfg(all(target_os = "macos", not(test)))]
#[path = "composer_menu.rs"]
mod menu;
#[path = "composer_native.rs"] mod native;
#[path = "composer_shortcuts.rs"] mod shortcuts;
#[path = "composer_text.rs"] mod text;

use std::{ops::Range, time::Instant};

use gpui::{
	AccessibleAction, AnyElement, App, Bounds, ClipboardItem, Context, CursorStyle, Element,
	ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
	Focusable, GlobalElementId, InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton,
	MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction, Pixels, Point, Render, Role,
	ScrollEvent, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine,
	fill,
	prelude::{
		FluentBuilder, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled,
	},
	relative,
};
#[cfg(target_os = "macos")] use objc2_app_kit::NSView;
#[cfg(target_os = "macos")] use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{
	ui_motion,
	ui_theme::{self, BODY_LINE_HEIGHT, BODY_SIZE, CONTROL_INSET, FIELD_MATERIAL},
};
use cursor::Cursor;
use decodex_protocol::PromptDraft;
use edit::{Edit, EditGroup, Snapshot};
use native::MAX_NATIVE_EDITOR_BYTES;
use text::ComposerTextElement;

gpui::actions!(
	decodex_composer_input,
	[
		Backspace,
		Delete,
		Left,
		Right,
		SelectLeft,
		SelectRight,
		SelectAll,
		InsertNewline,
		ShowCharacterPalette,
		Paste,
		PastePlainText,
		Cut,
		Copy,
		Undo,
		Redo,
		SubmitComposer,
	]
);

pub(crate) const MAX_COMPOSER_BYTES: usize = 16 * 1_024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ComposerEvent {
	Changed,
	Attach(ClipboardItem),
	AcceptSuggestion,
	RefreshSuggestion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComposerAppearance {
	Message,
	Workbench,
	Field,
	Search,
}

impl ComposerAppearance {
	fn multiline(self) -> bool {
		matches!(self, Self::Message | Self::Workbench)
	}
}

#[derive(Clone, Copy)]
enum SelectionUnit {
	Word,
	Line,
}

enum MouseSelection {
	Character,
	Unit { unit: SelectionUnit, anchor: Range<usize> },
}

/// Conversation composer input and its native text-input lifecycle.
pub(crate) struct ComposerInput {
	presentation: Entity<crate::ui_frame::Presentation<Self>>,
	cursor: Cursor,
	focus_handle: FocusHandle,
	placeholder: SharedString,
	placeholder_hint: Option<SharedString>,
	placeholder_hint_busy: bool,
	aria_label: SharedString,
	content: String,
	text_reveal: Option<crate::ui_motion::TextReveal>,
	visible_end: Option<usize>,
	native_part: Option<PromptDraft>,
	selected_range: Range<usize>,
	selection_reversed: bool,
	caret_after_wrap: bool,
	vertical_x: Option<Pixels>,
	marked_range: Option<Range<usize>>,
	last_layout: Option<Vec<WrappedLine>>,
	last_bounds: Option<Bounds<Pixels>>,
	text_offset: Pixels,
	scroll_manually: bool,
	mouse_selection: Option<MouseSelection>,
	appearance: ComposerAppearance,
	secret: bool,
	undo: Vec<Snapshot>,
	redo: Vec<Snapshot>,
	edit_group: Option<(EditGroup, Instant)>,
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

	pub(crate) fn message(
		tab_index: isize,
		placeholder: impl Into<SharedString>,
		aria_label: impl Into<SharedString>,
		cx: &mut Context<Self>,
	) -> Self {
		Self::build(tab_index, placeholder, aria_label, ComposerAppearance::Message, cx)
	}

	pub(crate) fn with_placeholder(
		tab_index: isize,
		placeholder: impl Into<SharedString>,
		aria_label: impl Into<SharedString>,
		cx: &mut Context<Self>,
	) -> Self {
		Self::build(tab_index, placeholder, aria_label, ComposerAppearance::Field, cx)
	}

	/// A search surface owns its outline; the text field keeps native editing behavior.
	pub(crate) fn search(
		placeholder: impl Into<SharedString>,
		aria_label: impl Into<SharedString>,
		cx: &mut Context<Self>,
	) -> Self {
		Self::build(0, placeholder, aria_label, ComposerAppearance::Search, cx)
	}

	fn build(
		tab_index: isize,
		placeholder: impl Into<SharedString>,
		aria_label: impl Into<SharedString>,
		appearance: ComposerAppearance,
		cx: &mut Context<Self>,
	) -> Self {
		let owner = cx.entity();
		let presentation = crate::ui_frame::Presentation::new(&owner, Self::render_input, cx);

		Self {
			presentation,
			cursor: Default::default(),
			focus_handle: cx.focus_handle().tab_index(tab_index).tab_stop(true),
			placeholder: placeholder.into(),
			placeholder_hint: None,
			placeholder_hint_busy: false,
			aria_label: aria_label.into(),
			content: String::new(),
			text_reveal: None,
			visible_end: None,
			native_part: None,
			selected_range: 0..0,
			selection_reversed: false,
			caret_after_wrap: false,
			vertical_x: None,
			marked_range: None,
			last_layout: None,
			last_bounds: None,
			text_offset: gpui::px(0.0),
			scroll_manually: false,
			mouse_selection: None,
			appearance,
			secret: false,
			undo: vec![],
			redo: vec![],
			edit_group: None,
		}
	}

	pub(crate) fn obscure(&mut self) {
		self.secret = true;
	}

	pub(crate) fn set_placeholder(
		&mut self,
		text: impl Into<SharedString>,
		cx: &mut Context<Self>,
	) {
		let text = text.into();

		if self.placeholder != text {
			self.placeholder = text;

			cx.notify();
		}
	}

	pub(crate) fn is_composing(&self) -> bool {
		self.marked_range.is_some()
	}

	pub(crate) fn set_placeholder_hint(
		&mut self,
		hint: Option<SharedString>,
		busy: bool,
		cx: &mut Context<Self>,
	) {
		if self.placeholder_hint != hint || self.placeholder_hint_busy != busy {
			self.placeholder_hint = hint;
			self.placeholder_hint_busy = busy;
			cx.notify();
		}
	}

	pub(crate) fn placeholder_hint(&self) -> Option<&str> {
		self.content.is_empty().then_some(self.placeholder_hint.as_deref()).flatten()
	}

	#[cfg(test)]
	pub(crate) fn placeholder(&self) -> &str {
		&self.placeholder
	}

	pub(crate) fn content(&self) -> &str {
		&self.content
	}

	pub(crate) fn len(&self) -> usize {
		self.content.len()
	}

	pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
		if self.content.is_empty() && self.native_part.is_none() {
			return;
		}

		self.content.clear();
		self.cursor.reset_motion();

		self.native_part = None;

		self.undo.clear();
		self.redo.clear();
		self.edit_group = None;

		self.selected_range = 0..0;
		self.selection_reversed = false;
		self.caret_after_wrap = false;
		self.marked_range = None;
		self.last_layout = None;

		self.changed(cx);
	}

	pub(crate) fn set_content(&mut self, value: &str, cx: &mut Context<Self>) {
		if self.content == value && self.native_part.is_none() {
			return;
		}

		self.native_part = None;

		self.cursor.reset_motion();
		self.replace_bytes(0..self.content.len(), value, Edit::Replace, cx);
		self.undo.clear();
		self.redo.clear();
	}

	/// Append recovered user text as one undoable edit, without truncation or automatic send.
	pub(crate) fn append_recovered_text(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
		let separator = if self.content.is_empty() { "" } else { "\n\n" };
		let appended = format!("{separator}{text}");
		if self.native_part.is_some() || self.content.len() + appended.len() > MAX_COMPOSER_BYTES {
			return false;
		}
		let end = self.content.len();
		self.replace_bytes(end..end, &appended, Edit::Replace, cx);
		true
	}

	pub(crate) fn set_streaming_content(&mut self, value: &str, cx: &mut Context<Self>) {
		if self.content == value {
			return;
		}

		let reveal = self.text_reveal.take().unwrap_or_else(|| {
			crate::ui_motion::TextReveal::settled(&self.content, Instant::now())
		});

		self.set_content(value, cx);

		self.text_reveal = Some(reveal);
	}

	fn visible_content(&self) -> &str {
		&self.content[..self.visible_end.unwrap_or(self.content.len())]
	}

	fn changed(&mut self, cx: &mut Context<Self>) {
		self.scroll_manually = false;
		self.mouse_selection = None;
		self.vertical_x = None;
		self.text_reveal = None;
		self.visible_end = None;

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
		let offset = if self.selected_range.is_empty() {
			next_boundary(&self.content, self.cursor_offset())
		} else {
			self.selected_range.end
		};
		self.apply_caret((offset, true), false, cx);
	}

	fn vertical(&mut self, direction: f32, extend: bool, cx: &mut Context<Self>) {
		if let Some(lines) = &self.last_layout {
			let position = text::caret_position(lines, self.cursor_offset(), self.caret_after_wrap);
			let goal = self.vertical_x.unwrap_or(position.x);
			let y = position.y + gpui::px(BODY_LINE_HEIGHT * direction);
			let caret = if y < gpui::px(0.) {
				(0, false)
			} else {
				text::caret_at(lines, gpui::point(goal, y))
			};

			self.apply_caret(caret, extend, cx);
			self.vertical_x = Some(goal);
		}
	}

	fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
		self.select_to(previous_boundary(&self.content, self.cursor_offset()), cx);
	}

	fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
		self.apply_caret((next_boundary(&self.content, self.cursor_offset()), true), true, cx);
	}

	fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
		self.set_selection(0, self.content.len(), cx);
		self.scroll_manually = true;
	}

	fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
		let previous = previous_boundary(&self.content, self.cursor_offset());
		if !self.delete_to(previous, Some(EditGroup::Backspace), cx) {
			window.play_system_bell();
		}
	}

	fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
		let next = next_boundary(&self.content, self.cursor_offset());
		if !self.delete_to(next, Some(EditGroup::Delete), cx) {
			window.play_system_bell();
		}
	}

	fn delete_to(
		&mut self,
		offset: usize,
		group: Option<EditGroup>,
		cx: &mut Context<Self>,
	) -> bool {
		let selection = self.replacement_range(None);
		let (range, edit) = if selection.is_empty() {
			let cursor = selection.start;
			let offset = self.content.floor_char_boundary(offset.min(self.content.len()));
			(cursor.min(offset)..cursor.max(offset), group.map_or(Edit::Replace, Edit::Group))
		} else {
			(selection, Edit::Replace)
		};
		!range.is_empty() && self.replace_bytes(range, "", edit, cx)
	}

	fn insert_newline(&mut self, _: &InsertNewline, _: &mut Window, cx: &mut Context<Self>) {
		self.replace_bytes(self.replacement_range(None), "\n", Edit::Replace, cx);
	}

	fn show_character_palette(
		&mut self,
		_: &ShowCharacterPalette,
		window: &mut Window,
		_: &mut Context<Self>,
	) {
		window.show_character_palette();
	}

	fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(item) = cx.read_from_clipboard() {
			self.paste_clipboard(item, false, cx);
		}
	}

	fn paste_plain_text(&mut self, _: &PastePlainText, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(item) = cx.read_from_clipboard() {
			self.paste_clipboard(item, true, cx);
		}
	}

	fn paste_clipboard(&mut self, item: ClipboardItem, plain: bool, cx: &mut Context<Self>) {
		if !plain
			&& self.appearance.multiline()
			&& item.entries.iter().any(|entry| !matches!(entry, gpui::ClipboardEntry::String(_)))
		{
			cx.emit(ComposerEvent::Attach(item));
		} else if let Some(text) = item.text() {
			self.replace_bytes(self.replacement_range(None), &text, Edit::Replace, cx);
		}
	}

	fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
		if self.secret {
			return;
		}
		if !self.selected_range.is_empty() {
			cx.write_to_clipboard(ClipboardItem::new_string(
				self.content[self.selected_range.clone()].to_owned(),
			));
		}
	}

	fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
		if self.secret || self.selected_range.is_empty() {
			return;
		}

		let range = self.selected_range.clone();
		let text = self.content[range.clone()].to_owned();
		if self.replace_bytes(range, "", Edit::Replace, cx) {
			cx.write_to_clipboard(ClipboardItem::new_string(text));
		}
	}

	fn on_mouse_down(
		&mut self,
		event: &MouseDownEvent,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		#[cfg(target_os = "macos")]
		claim_native_text_focus(window);

		self.cursor.reset();
		window.focus(&self.focus_handle, cx);

		self.mouse_selection = Some(MouseSelection::Character);
		if event.modifiers.shift {
			self.apply_caret(self.caret_for_mouse_position(event.position), true, cx);
			return;
		}

		let unit = match event.click_count {
			2 => SelectionUnit::Word,
			3.. => SelectionUnit::Line,
			_ => {
				self.apply_caret(self.caret_for_mouse_position(event.position), false, cx);
				return;
			},
		};
		let anchor = self.mouse_selection_range(event.position, unit);
		self.set_selection(anchor.start, anchor.end, cx);
		self.mouse_selection = Some(MouseSelection::Unit { unit, anchor });
	}

	fn on_context_menu(
		&mut self,
		event: &MouseDownEvent,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		#[cfg(target_os = "macos")]
		claim_native_text_focus(window);
		window.focus(&self.focus_handle, cx);
		self.cursor.reset();
		self.mouse_selection = None;
		self.edit_group = None;
		let glyph = self
			.text_hit(event.position)
			.and_then(|(lines, point)| text::character_at(lines, point));
		if !glyph.is_some_and(|glyph| self.selected_range.contains(&glyph)) {
			if self.secret {
				self.move_to(self.index_for_mouse_position(event.position), cx);
			} else {
				let range = self.mouse_selection_range(event.position, SelectionUnit::Word);
				self.set_selection(range.start, range.end, cx);
			}
		}

		#[cfg(all(target_os = "macos", not(test)))]
		menu::show(self, window, cx);
		cx.stop_propagation();
	}

	fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
		self.mouse_selection = None;
	}

	fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
		if event.pressed_button != Some(MouseButton::Left) {
			self.mouse_selection = None;
			return;
		}

		let position = self.scroll_selection_drag(event.position);
		let scrolled = position != event.position;
		match &self.mouse_selection {
			Some(MouseSelection::Character) =>
				self.apply_caret(self.caret_for_mouse_position(position), true, cx),
			Some(MouseSelection::Unit { unit, anchor }) => {
				let target = self.mouse_selection_range(position, *unit);
				let (anchor, head) = if target.start < anchor.start {
					(anchor.end, target.start)
				} else {
					(anchor.start, anchor.end.max(target.end))
				};
				self.set_selection(anchor, head, cx);
			},
			None => (),
		}
		if scrolled {
			self.scroll_manually = true;
		}
	}

	fn scroll_selection_drag(&mut self, mut position: Point<Pixels>) -> Point<Pixels> {
		if let (Some(bounds), Some(lines)) = (self.last_bounds, &self.last_layout) {
			let step = gpui::px(BODY_LINE_HEIGHT);
			let max = (text::height(lines) - bounds.size.height).max(gpui::px(0.));
			if position.y < bounds.top() && self.text_offset > gpui::px(0.) {
				self.text_offset = (self.text_offset - step).max(gpui::px(0.));
				position.y = bounds.top();
			} else if position.y >= bounds.bottom() && self.text_offset < max {
				self.text_offset = (self.text_offset + step).min(max);
				position.y = bounds.bottom() - gpui::px(1.);
			}
		}
		position
	}

	fn mouse_selection_range(&self, position: Point<Pixels>, unit: SelectionUnit) -> Range<usize> {
		if self.secret {
			return 0..self.content.len();
		}

		if let Some((lines, position)) = self.text_hit(position) {
			let range = match unit {
				SelectionUnit::Word => text::character_at(lines, position)
					.and_then(|index| word_at(&self.content, index)),
				SelectionUnit::Line => text::row_range_at(lines, position),
			};
			if let Some(range) = range {
				return range;
			}
		}

		let offset = self.index_for_mouse_position(position);
		offset..offset
	}

	fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
		self.set_selection(offset, offset, cx);
	}

	fn apply_caret(
		&mut self,
		(offset, after_wrap): (usize, bool),
		extend: bool,
		cx: &mut Context<Self>,
	) {
		if extend {
			self.select_to(offset, cx);
		} else {
			self.move_to(offset, cx);
		}
		self.caret_after_wrap = after_wrap;
	}

	fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
		let anchor = if self.selection_reversed {
			self.selected_range.end
		} else {
			self.selected_range.start
		};
		self.set_selection(anchor, offset, cx);
	}

	fn set_selection(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
		let boundary =
			|offset: usize| self.content.floor_char_boundary(offset.min(self.content.len()));
		let (anchor, head) = (boundary(anchor), boundary(head));
		self.selected_range = anchor.min(head)..anchor.max(head);
		self.selection_reversed = head < anchor;
		self.caret_after_wrap = false;
		self.vertical_x = None;
		self.scroll_manually = false;
		self.marked_range = None;
		self.edit_group = None;

		cx.notify();
	}

	fn cursor_offset(&self) -> usize {
		if self.selection_reversed { self.selected_range.start } else { self.selected_range.end }
	}

	fn text_position(&self, position: Point<Pixels>, origin: Point<Pixels>) -> Point<Pixels> {
		position - origin + gpui::point(gpui::px(0.0), self.text_offset)
	}

	fn text_hit(&self, position: Point<Pixels>) -> Option<(&[WrappedLine], Point<Pixels>)> {
		let bounds = self.last_bounds?;
		(!self.content.is_empty() && position.y >= bounds.top() && position.y < bounds.bottom())
			.then_some((self.last_layout.as_deref()?, self.text_position(position, bounds.origin)))
	}

	fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
		self.caret_for_mouse_position(position).0
	}

	fn caret_for_mouse_position(&self, position: Point<Pixels>) -> (usize, bool) {
		if self.content.is_empty() {
			return (0, false);
		}

		let (Some(bounds), Some(lines)) = (self.last_bounds, self.last_layout.as_deref()) else {
			return (self.cursor_offset(), self.caret_after_wrap);
		};

		if position.y < bounds.top() {
			return (0, false);
		}
		if position.y >= bounds.bottom() {
			return (self.content.len(), false);
		}

		text::caret_at(lines, self.text_position(position, bounds.origin))
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
		edit: Edit,
		cx: &mut Context<Self>,
	) -> bool {
		let retained = self.content.len().saturating_sub(range.end.saturating_sub(range.start));
		let maximum =
			if self.native_part.is_some() { MAX_NATIVE_EDITOR_BYTES } else { MAX_COMPOSER_BYTES };
		let replacement = bounded_input(new_text, maximum.saturating_sub(retained));
		let mark = matches!(edit, Edit::Marked(_));
		let mut native_part = self.native_part.clone();

		if let Some(part) = &mut native_part {
			// Native restoration never truncates an edit or strips a bound marker.
			if replacement != new_text || part.replace_text(0, range.clone(), new_text).is_err() {
				return false;
			}
		}

		self.checkpoint(&edit);

		self.native_part = native_part;

		let inserted = range.start..range.start + replacement.len();
		let relative_selection = match edit {
			Edit::Marked(Some(selection)) => Some(range_from_utf16(&replacement, &selection)),
			_ => None,
		};

		self.content.replace_range(range, &replacement);

		self.marked_range = mark.then(|| inserted.clone()).filter(|range| !range.is_empty());
		self.selected_range = relative_selection.map_or_else(
			|| inserted.end..inserted.end,
			|selection| inserted.start + selection.start..inserted.start + selection.end,
		);
		self.selection_reversed = false;
		self.caret_after_wrap = true;
		self.last_layout = None;

		self.changed(cx);
		true
	}

	fn set_accessible_value(
		&mut self,
		data: Option<&gpui::accesskit::ActionData>,
		cx: &mut Context<Self>,
	) {
		let Some(gpui::accesskit::ActionData::Value(value)) = data else {
			return;
		};

		self.replace_bytes(0..self.content.len(), value, Edit::Replace, cx);
		self.undo.clear();
		self.redo.clear();
	}
}

impl ComposerInput {
	fn render_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		if let Some(reveal) = &mut self.text_reveal {
			let (end, moving) = reveal.sample(&self.content, Instant::now());

			self.visible_end = moving.then_some(end);

			if moving {
				ui_motion::request_frame(window, cx);
			} else {
				self.text_reveal = None;
			}
		}

		self.update_cursor(window, cx);

		let entity = cx.entity();
		let focus_handle = self.focus_handle.clone();
		let multiline = self.appearance.multiline();
		let search = self.appearance == ComposerAppearance::Search;
		let suggestion = self.placeholder_hint();

		gpui::div()
			.id("conversation-composer-input")
			.debug_selector(move || {
				if multiline { "composer-input-text" } else { "field-input-text" }.into()
			})
			.key_context("ComposerInput")
			.role(Role::TextInput)
			.aria_label(if let Some(hint) = suggestion {
				format!("{}. Press {hint} to use the suggestion.", self.aria_label).into()
			} else {
				self.aria_label.clone()
			})
			.aria_placeholder(self.placeholder.clone())
			.aria_value(if self.secret {
				"*".repeat(self.content.len())
			} else {
				self.content.clone()
			})
			.track_focus(&focus_handle)
			.on_a11y_action(AccessibleAction::SetValue, {
				let entity = entity.clone();

				move |data, _, cx| {
					entity.update(cx, |input, cx| input.set_accessible_value(data, cx));
				}
			})
			.on_action(cx.listener(Self::undo))
			.map(|input| shortcuts::bind_actions(input, cx))
			.on_action(cx.listener(Self::redo))
			.on_action(cx.listener(Self::backspace))
			.on_action(cx.listener(Self::delete))
			.on_action(cx.listener(Self::left))
			.on_action(cx.listener(Self::right))
			.on_action(cx.listener(Self::select_left))
			.on_action(cx.listener(Self::select_right))
			.on_action(cx.listener(Self::select_all))
			.on_action(cx.listener(Self::insert_newline))
			.on_action(cx.listener(Self::show_character_palette))
			.on_action(cx.listener(Self::paste))
			.on_action(cx.listener(Self::paste_plain_text))
			.on_action(cx.listener(Self::cut))
			.on_action(cx.listener(Self::copy))
			.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
				if !s.is_composing() {
					cx.propagate();
				}
			}))
			.on_key_down(cx.listener(|s, _, _, cx| {
				s.cursor.reset();
				cx.notify();
			}))
			.on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
			.on_mouse_down(MouseButton::Right, cx.listener(Self::on_context_menu))
			.on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
			.on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
			.on_scroll(cx.listener(|s, e: &ScrollEvent, _, cx| {
				if s.appearance.multiline() {
					s.text_offset = (s.text_offset
						- e.delta.pixel_delta(gpui::px(BODY_LINE_HEIGHT)).y)
						.max(gpui::px(0.0));
					s.scroll_manually = true;

					cx.stop_propagation();
					cx.notify();
				}
			}))
			.cursor(CursorStyle::IBeam)
			.w_full()
			.when(!multiline, |d| d.h_full())
			.px(gpui::px(if search { 0. } else { 8. }))
			.py(gpui::px(if multiline { CONTROL_INSET } else { 1.5 }))
			.when(self.appearance == ComposerAppearance::Message, |d| {
				d.px(gpui::px(ui_theme::COMPOSER_TEXT_INSET))
					.pt(gpui::px(ui_theme::COMPOSER_TEXT_TOP_INSET))
					.pb(gpui::px(ui_theme::COMPOSER_TEXT_BOTTOM_INSET))
			})
			.flex()
			.items_start()
			.when(!multiline, |d| d.items_center())
			.overflow_hidden()
			.rounded(gpui::px(if multiline { 8.0 } else { ui_theme::SETTINGS_CONTROL_RADIUS }))
			.border(gpui::px(if search || self.appearance == ComposerAppearance::Message {
				0.
			} else if multiline {
				1.0
			} else {
				0.5
			}))
			.border_color(if multiline || search {
				ui_theme::rgba(0x00000000)
			} else if focus_handle.is_focused(window) {
				ui_theme::rgb(ui_theme::ACCENT)
			} else {
				ui_theme::rgba(ui_theme::PANEL_DIVIDER)
			})
			.bg(if multiline || search {
				ui_theme::rgba(0x00000000)
			} else {
				ui_theme::rgba(FIELD_MATERIAL)
			})
			.text_size(gpui::px(BODY_SIZE))
			.text_color(ui_theme::rgb(if multiline { 0xeeeaf0 } else { ui_theme::TEXT }))
			.child(ComposerTextElement { input: entity })
			.into_any_element()
	}
}

impl EventEmitter<ComposerEvent> for ComposerInput {}

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

	fn paste(&mut self, item: ClipboardItem, _: &mut Window, cx: &mut Context<Self>) {
		self.paste_clipboard(item, false, cx);
	}

	fn replace_text_in_range(
		&mut self,
		range_utf16: Option<Range<usize>>,
		new_text: &str,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		let range = self.replacement_range(range_utf16.as_ref());
		let edit = if range_utf16.is_none() && !new_text.is_empty() && !new_text.contains('\n') {
			Edit::Group(EditGroup::Typing)
		} else {
			Edit::Replace
		};
		self.replace_bytes(range, new_text, edit, cx);
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

		self.replace_bytes(range, new_text, Edit::Marked(new_selected_range_utf16), cx);
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
		let start = text::caret_position(
			line,
			range.start,
			range.start == self.cursor_offset() && self.caret_after_wrap,
		);
		let end = text::position_at(line, range.end);
		let origin = self.last_bounds.unwrap_or(bounds).origin
			- gpui::point(gpui::px(0.0), self.text_offset);

		Some(Bounds::new(
			origin + start,
			gpui::size((end.x - start.x).max(gpui::px(1.0)), gpui::px(BODY_LINE_HEIGHT)),
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
		let range = range_from_utf16(&self.content, &range_utf16);
		self.set_selection(range.start, range.end, cx);
	}

	fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
		Some(offset_to_utf16(&self.content, self.content.len()))
	}
}

impl Render for ComposerInput {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		self.presentation.clone()
	}
}

pub(crate) fn bind_keys(cx: &mut App) {
	shortcuts::bind_keys(cx);

	cx.bind_keys([
		KeyBinding::new("backspace", Backspace, Some("ComposerInput")),
		KeyBinding::new("delete", Delete, Some("ComposerInput")),
		KeyBinding::new("left", Left, Some("ComposerInput")),
		KeyBinding::new("right", Right, Some("ComposerInput")),
		KeyBinding::new("shift-left", SelectLeft, Some("ComposerInput")),
		KeyBinding::new("shift-right", SelectRight, Some("ComposerInput")),
		KeyBinding::new("cmd-a", SelectAll, Some("ComposerInput")),
		KeyBinding::new("shift-enter", InsertNewline, Some("ComposerInput")),
		KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some("ComposerInput")),
		KeyBinding::new("cmd-v", Paste, Some("ComposerInput")),
		KeyBinding::new("alt-cmd-shift-v", PastePlainText, Some("ComposerInput")),
		KeyBinding::new("cmd-x", Cut, Some("ComposerInput")),
		KeyBinding::new("cmd-c", Copy, Some("ComposerInput")),
		KeyBinding::new("enter", SubmitComposer, Some("ComposerInput")),
		KeyBinding::new("cmd-enter", SubmitComposer, Some("ComposerInput")),
		KeyBinding::new("enter", NoAction, Some("AsyncQuestion > ComposerInput")),
		KeyBinding::new("cmd-enter", NoAction, Some("AsyncQuestion > ComposerInput")),
		KeyBinding::new("cmd-z", Undo, Some("ComposerInput")),
		KeyBinding::new("cmd-shift-z", Redo, Some("ComposerInput")),
	]);
}

// GPUI focus and AppKit's first responder are separate. A pointer click into
// the editor must reclaim the native text client after another native view used it.
#[cfg(target_os = "macos")]
fn claim_native_text_focus(window: &Window) {
	let Ok(handle) = HasWindowHandle::window_handle(window) else {
		return;
	};
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
		return;
	};
	// The live GPUI window owns this view; mouse dispatch runs on the main thread.
	let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };

	if let Some(native) = view.window() {
		native.makeFirstResponder(Some(view));
	}
}

fn previous_boundary(content: &str, offset: usize) -> usize {
	content[..offset.min(content.len())]
		.grapheme_indices(true)
		.next_back()
		.map_or(0, |(index, _)| index)
}

fn next_boundary(content: &str, offset: usize) -> usize {
	let offset = offset.min(content.len());

	content[offset..]
		.graphemes(true)
		.next()
		.map_or(content.len(), |grapheme| offset + grapheme.len())
}

#[cfg(target_os = "macos")]
fn native_word_text(content: &str) -> objc2::rc::Retained<objc2_foundation::NSAttributedString> {
	use objc2::AnyThread as _;
	use objc2_foundation::{NSAttributedString, NSString};

	NSAttributedString::initWithString(NSAttributedString::alloc(), &NSString::from_str(content))
}

#[cfg(target_os = "macos")]
fn word_at(content: &str, offset: usize) -> Option<Range<usize>> {
	use objc2_app_kit::NSAttributedStringAppKitAdditions as _;

	if offset >= content.len() {
		return None;
	}

	let text = native_word_text(content);
	let range = text.doubleClickAtIndex(offset_to_utf16(content, offset));
	Some(range_from_utf16(content, &(range.location..range.location + range.length)))
}

#[cfg(target_os = "macos")]
fn word_boundary(content: &str, offset: usize, forward: bool) -> usize {
	use objc2_app_kit::NSAttributedStringAppKitAdditions as _;

	let index = native_word_text(content)
		.nextWordFromIndex_forward(offset_to_utf16(content, offset), forward);
	offset_from_utf16(content, index)
}

#[cfg(not(target_os = "macos"))]
fn word_boundary(content: &str, offset: usize, forward: bool) -> usize {
	if forward {
		content[offset..]
			.unicode_word_indices()
			.next()
			.map_or(content.len(), |(i, word)| offset + i + word.len())
	} else {
		content[..offset].unicode_word_indices().next_back().map_or(0, |(i, _)| i)
	}
}

#[cfg(not(target_os = "macos"))]
fn word_at(content: &str, offset: usize) -> Option<Range<usize>> {
	content.split_word_bound_indices().find_map(|(start, word)| {
		let range = start..start + word.len();
		range.contains(&offset).then_some(range)
	})
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
		if character.is_control() && !matches!(character, '\n' | '\t') {
			continue;
		}
		if output.len().saturating_add(character.len_utf8()) > maximum_bytes {
			break;
		}

		output.push(character);
	}

	output
}

#[cfg(test)]
mod multiline_tests {
	use gpui::{
		EntityInputHandler as _, Focusable as _, MouseButton, MouseDownEvent, Pixels, Point,
		VisualTestContext,
	};

	use crate::composer_input::{
		self, ClipboardItem, ComposerInput, Edit, Paste, Redo, Undo, bind_keys, text, ui_theme,
	};

	#[gpui::test]
	fn recovered_answers_append_as_one_undoable_edit(cx: &mut gpui::TestAppContext) {
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		visual.update(|window, cx| {
			input.update(cx, |input, cx| {
				input.set_content("Existing", cx);
				assert!(input.append_recovered_text("First\n\nSecond", cx));
				assert_eq!(input.content(), "Existing\n\nFirst\n\nSecond");
				input.undo(&Undo, window, cx);
				assert_eq!(input.content(), "Existing");
				input.redo(&Redo, window, cx);
				assert_eq!(input.content(), "Existing\n\nFirst\n\nSecond");
			})
		});
	}

	#[gpui::test]
	fn pasted_tabs_preserve_indentation_and_undo(cx: &mut gpui::TestAppContext) {
		cx.update(bind_keys);

		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));

		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			cx.write_to_clipboard(ClipboardItem::new_string("界\tfirst\r\n\t\tsecond\n".into()));

			input.update(cx, |input, cx| {
				input.paste(&Paste, window, cx);

				let expected = "界\tfirst\n\t\tsecond\n";

				assert_eq!(input.content(), expected);

				input.undo(&Undo, window, cx);

				assert_eq!(input.content(), "");

				input.redo(&Redo, window, cx);

				assert_eq!(input.content(), expected);
				assert_eq!(decodex_protocol::WireText::new(expected).unwrap().as_str(), expected);
			});
		});

		assert_eq!(composer_input::bounded_input("界\t\u{0}\u{1b}文", 5), "界\t");
	}

	#[gpui::test]
	fn typing_cut_paste_and_undo_preserve_selection(cx: &mut gpui::TestAppContext) {
		cx.update(bind_keys);
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		input.update(visual, |input, cx| input.set_content("Some ", cx));
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			crate::ui_frame::draw_current_frame(window, cx);
		});

		for character in ["t", "e", "x", "t"] {
			visual.simulate_input(character);
		}
		visual.simulate_keystrokes("cmd-z");
		input.read_with(visual, |input, _| assert_eq!(input.content(), "Some "));
		visual.simulate_keystrokes("cmd-shift-z alt-shift-left cmd-x");
		input.read_with(visual, |input, cx| {
			assert_eq!(input.content(), "Some ");
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
				Some("text")
			);
		});
		visual.simulate_keystrokes("cmd-z");
		input.read_with(visual, |input, _| {
			assert_eq!(input.content(), "Some text");
			assert_eq!(input.selected_range, 5..9);
			assert!(input.selection_reversed);
		});
		visual.simulate_keystrokes("cmd-shift-z alt-cmd-shift-v");
		input.read_with(visual, |input, _| assert_eq!(input.content(), "Some text"));

		visual.simulate_keystrokes("backspace backspace cmd-z");
		input.read_with(visual, |input, _| {
			assert_eq!(input.content(), "Some text");
			assert_eq!(input.selected_range, 9..9);
		});
	}

	#[gpui::test]
	fn ordinary_draft_preserves_composition_undo_and_redo(cx: &mut gpui::TestAppContext) {
		cx.update(bind_keys);

		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));

		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			input.update(cx, |input, cx| {
				input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
				input.replace_and_mark_text_in_range(None, "你", Some(1..1), window, cx);
				input.replace_text_in_range(None, "你", window, cx);

				assert_eq!(input.content(), "你");

				input.undo(&Undo, window, cx);

				assert_eq!(input.content(), "");

				input.redo(&Redo, window, cx);

				assert_eq!(input.content(), "你");
			});
		});
	}

	#[gpui::test]
	fn streamed_draft_keeps_complete_text_and_manual_edits_end_reveal(
		cx: &mut gpui::TestAppContext,
	) {
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		let text = "你好👨‍👩‍👧‍👦".repeat(60);

		input.update(visual, |input, cx| input.set_streaming_content(&text, cx));
		visual.update(crate::ui_frame::draw_current_frame);
		input.update(visual, |input, cx| {
			assert_eq!(
				input.content(),
				text,
				"saving and submitting never use the animated prefix"
			);
			assert!(input.visible_content().len() < text.len());

			input.replace_bytes(0..0, "Edit: ", Edit::Replace, cx);

			assert!(input.text_reveal.is_none());
			assert_eq!(input.visible_content(), input.content());
			assert_eq!(input.content(), format!("Edit: {text}"));
		});
	}

	#[gpui::test]
	fn measured_height_matches_painted_wrapping(cx: &mut gpui::TestAppContext) {
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));

		input.update(visual, |input, cx| {
			input.set_content(
				"Wait, wait, wait, you are a  Wait, wait, 一二三四五六你好，你好能听到吗？你好",
				cx,
			)
		});

		for width in [460., 500., 600., 740.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(300.)));
			visual.update(crate::ui_frame::draw_current_frame);

			input.read_with(visual, |input, _| {
				let painted: gpui::Pixels = input
					.last_layout
					.as_ref()
					.unwrap()
					.iter()
					.map(|line| line.size(gpui::px(ui_theme::BODY_LINE_HEIGHT)).height)
					.sum();

				assert_eq!(
					input.last_bounds.unwrap().size.height,
					painted,
					"measurement must not reserve a blank row at width {width}"
				);
			});
		}
	}

	#[gpui::test]
	fn suggestion_actions_wrap_with_trailing_text_at_full_width(cx: &mut gpui::TestAppContext) {
		let (input, visual) =
			cx.add_window_view(|_, cx| ComposerInput::message(0, "", "Message", cx));
		let tail = "Compare all earlier options before choosing the next step.";
		let draft = format!("{tail} {tail}\n{tail}");
		input.update(visual, |input, cx| input.set_placeholder(draft, cx));
		visual.update(crate::ui_frame::draw_current_frame);
		let width = visual.update(|window, cx| {
			let input = input.read(cx);
			let inset = window.viewport_size().width - input.last_bounds.unwrap().size.width;
			let last = input.last_layout.as_ref().unwrap().last().unwrap();
			last.unwrapped_layout.width.ceil() + inset + gpui::px(12.)
		});
		visual.simulate_resize(gpui::size(width, gpui::px(300.)));
		visual.update(crate::ui_frame::draw_current_frame);
		let (plain_width, first_wraps) = input.read_with(visual, |input, _| {
			let lines = input.last_layout.as_ref().unwrap();
			assert!(!lines[0].wrap_boundaries().is_empty());
			assert!(lines[1].wrap_boundaries().is_empty());
			(input.last_bounds.unwrap().size.width, lines[0].wrap_boundaries().to_vec())
		});

		input.update(visual, |input, cx| input.set_placeholder_hint(Some("Tab".into()), false, cx));
		visual.update(crate::ui_frame::draw_current_frame);
		let accept = visual.debug_bounds("composer-suggestion-accept").unwrap();
		let keycap = visual.debug_bounds("composer-suggestion-keycap").unwrap();
		let refresh = visual.debug_bounds("composer-suggestion-refresh").unwrap();
		input.read_with(visual, |input, _| {
			let bounds = input.last_bounds.unwrap();
			let lines = input.last_layout.as_ref().unwrap();
			assert_eq!(bounds.size.width, plain_width);
			assert_eq!(lines[0].wrap_boundaries(), first_wraps);
			assert_eq!(lines.len(), 3, "only the trailing text needs another row");
			let tail = &lines[2];
			assert!(!tail.text.trim().is_empty());
			assert!(tail.wrap_boundaries().is_empty());
			let line_height = gpui::px(ui_theme::BODY_LINE_HEIGHT);
			assert!(
				(accept.center().y - (bounds.bottom() - line_height / 2.)).abs() <= gpui::px(1.)
			);
			let tail_end = bounds.left() + tail.unwrapped_layout.width;
			assert!(
				keycap.left() - tail_end > refresh.center().x - keycap.right(),
				"the keycap should sit closer to the refresh icon than to the suggestion text"
			);
		});
	}

	#[gpui::test]
	fn select_all_highlight_covers_each_wrapped_row(cx: &mut gpui::TestAppContext) {
		cx.update(bind_keys);
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		let draft = "这是需要自动换行的中文草稿，Wrap selection。";

		visual.simulate_resize(gpui::size(gpui::px(205.), gpui::px(300.)));
		input.update(visual, |input, cx| input.set_content(draft, cx));
		visual.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
		visual.simulate_keystrokes("cmd-a");
		visual.update(|window, cx| {
			crate::ui_frame::draw_current_frame(window, cx);

			let input = input.read(cx);
			assert_eq!(input.selected_range, 0..draft.len());
			let lines = input.last_layout.as_ref().unwrap();
			let [boundary] = lines[0].wrap_boundaries() else {
				panic!("expected two visual rows in a single paragraph");
			};
			let split = lines[0].runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index;
			let line_height = gpui::px(ui_theme::BODY_LINE_HEIGHT);
			let origin = input.last_bounds.unwrap().origin;
			let expected = [split, draft.len()].map(|end| {
				let edge = text::position_at(lines, end);
				gpui::Bounds::new(
					origin + gpui::point(gpui::px(0.), edge.y),
					gpui::size(edge.x, line_height),
				)
				.scale(window.scale_factor())
			});

			let paint = gpui::Background::from(ui_theme::rgba(0x60a5fa40));
			let highlights: Vec<_> = window
				.painted_quads()
				.into_iter()
				.filter(|quad| quad.background == paint)
				.map(|quad| quad.bounds)
				.collect();
			assert_eq!(highlights, expected);
		});
	}

	fn press(visual: &mut VisualTestContext, position: Point<Pixels>, click_count: usize) {
		visual.simulate_event(MouseDownEvent {
			position,
			button: MouseButton::Left,
			modifiers: Default::default(),
			click_count,
			first_mouse: false,
		});
	}

	#[cfg(target_os = "macos")]
	#[gpui::test]
	fn double_click_selects_native_words_and_drags_by_word(cx: &mut gpui::TestAppContext) {
		cx.update(bind_keys);
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		let draft = "alpha 你好 e\u{301} 👩‍💻";
		input.update(visual, |input, cx| input.set_content(draft, cx));
		visual.update(crate::ui_frame::draw_current_frame);
		let (alpha, chinese, emoji) = input.read_with(visual, |input, _| {
			let lines = input.last_layout.as_ref().unwrap();
			let origin = input.last_bounds.unwrap().origin;
			let glyph = |start, end| {
				let left = text::position_at(lines, start).x;
				let right = text::position_at(lines, end).x;
				origin + gpui::point(left + (right - left) * 0.75, gpui::px(5.))
			};
			(
				glyph("alph".len(), "alpha".len()),
				glyph("alpha 你".len(), "alpha 你好".len()),
				glyph("alpha 你好 e\u{301} ".len(), draft.len()),
			)
		});
		let check_selection = |visual: &mut VisualTestContext, expected, reversed| {
			input.read_with(visual, |input, _| {
				assert_eq!(&input.content[input.selected_range.clone()], expected);
				assert_eq!(input.selection_reversed, reversed);
			});
		};

		visual.simulate_click(alpha, Default::default());
		check_selection(visual, "", false);
		press(visual, alpha, 2);
		check_selection(visual, "alpha", false);
		visual.simulate_mouse_up(alpha, MouseButton::Left, Default::default());

		press(visual, chinese, 2);
		check_selection(visual, "你好", false);
		visual.simulate_mouse_move(alpha, MouseButton::Left, Default::default());
		check_selection(visual, "alpha 你好", true);
		visual.simulate_mouse_move(emoji, MouseButton::Left, Default::default());
		check_selection(visual, "你好 e\u{301} 👩‍💻", false);
		visual.simulate_mouse_up(emoji, MouseButton::Left, Default::default());
		visual.simulate_mouse_move(alpha, None, Default::default());
		visual.simulate_event(MouseDownEvent {
			position: chinese,
			button: MouseButton::Right,
			modifiers: Default::default(),
			click_count: 1,
			first_mouse: false,
		});
		check_selection(visual, "你好 e\u{301} 👩‍💻", false);
		visual.simulate_keystrokes("cmd-c");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
				Some("你好 e\u{301} 👩‍💻")
			);
		});
	}

	#[gpui::test]
	fn triple_click_selects_and_drags_by_visual_row(cx: &mut gpui::TestAppContext) {
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		let first_line = "这是需要自动换行的中文草稿，Wrap selection。";
		let draft = format!("{first_line}\nNext line");
		visual.simulate_resize(gpui::size(gpui::px(205.), gpui::px(300.)));
		input.update(visual, |input, cx| input.set_content(&draft, cx));
		visual.update(crate::ui_frame::draw_current_frame);
		let (points, split) = input.read_with(visual, |input, _| {
			let bounds = input.last_bounds.unwrap();
			let line = &input.last_layout.as_ref().unwrap()[0];
			let [boundary] = line.wrap_boundaries() else {
				panic!("expected two visual rows in the first line");
			};
			let split = line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index;
			let points = [0.5, 1.5, 2.5].map(|row| {
				gpui::point(
					bounds.right() - gpui::px(1.),
					bounds.top() + gpui::px(ui_theme::BODY_LINE_HEIGHT * row),
				)
			});
			(points, split)
		});

		press(visual, points[1], 3);
		visual.update(|window, cx| {
			crate::ui_frame::draw_current_frame(window, cx);
			assert_eq!(input.read(cx).selected_range, split..first_line.len());
			let paint = gpui::Background::from(ui_theme::rgba(0x60a5fa40));
			let highlights: Vec<_> = window
				.painted_quads()
				.into_iter()
				.filter(|quad| quad.background == paint)
				.collect();
			assert_eq!(highlights.len(), 1);
			let center = points[1].y.scale(window.scale_factor());
			assert!(highlights[0].bounds.top() < center && center < highlights[0].bounds.bottom());
		});
		visual.simulate_mouse_move(points[0], MouseButton::Left, Default::default());
		input.read_with(visual, |input, _| {
			assert_eq!(input.selected_range, 0..first_line.len());
			assert!(input.selection_reversed);
		});
		visual.simulate_mouse_move(points[2], MouseButton::Left, Default::default());
		input.read_with(visual, |input, _| {
			assert_eq!(input.selected_range, split..draft.len());
			assert!(!input.selection_reversed);
		});
		visual.simulate_mouse_up(points[2], MouseButton::Left, Default::default());
	}

	#[gpui::test]
	fn newline_wrap_and_native_caret_share_geometry(cx: &mut gpui::TestAppContext) {
		cx.update(bind_keys);

		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));

		visual.simulate_resize(gpui::size(gpui::px(240.0), gpui::px(300.0)));
		input.update(visual, |input, cx| input.set_content("第一行", cx));
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			window.draw(cx).clear(cx);
		});
		visual.simulate_keystrokes("shift-enter");
		visual.update(crate::ui_frame::draw_current_frame);

		input.update(visual, |input, cx| {
			assert_eq!(input.content(), "第一行\n");

			let lines = input.last_layout.as_ref().unwrap();

			assert_eq!(lines.len(), 2);
			assert_eq!(
				text::position_at(lines, input.content.len()).y,
				gpui::px(ui_theme::BODY_LINE_HEIGHT)
			);

			input.set_content(&"宽度有限，长段落必须自动换行。".repeat(40), cx);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		input.update(visual, |input, _| {
			let lines = input.last_layout.as_ref().unwrap();

			assert!(!lines[0].wrap_boundaries().is_empty());
			assert!(input.text_offset > gpui::px(0.0));

			let caret = text::position_at(lines, input.content.len());

			assert_eq!(text::index_at(lines, caret), input.content.len());
			assert!(
				input.last_bounds.unwrap().size.height
					<= gpui::px(ui_theme::BODY_LINE_HEIGHT * 6.0)
			);
		});
	}

	#[gpui::test]
	fn obscured_unicode_mouse_selection_keeps_valid_text_boundaries(cx: &mut gpui::TestAppContext) {
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));

		input.update(visual, |input, cx| {
			input.set_content("你abc", cx);
			input.obscure();
		});
		visual.update(crate::ui_frame::draw_current_frame);

		let position = input.read_with(visual, |input, _| {
			input.last_bounds.unwrap().origin
				+ text::position_at(input.last_layout.as_ref().unwrap(), 1)
		});

		input.update(visual, |input, cx| {
			input.move_to(input.index_for_mouse_position(position), cx);
		});
		input.read_with(visual, |input, _| {
			assert!(input.content.is_char_boundary(input.selected_range.start));
			assert!(input.content.is_char_boundary(input.selected_range.end));
		});
		input.update(visual, |input, cx| {
			input.select_to(input.index_for_mouse_position(position), cx);

			assert!(input.content.is_char_boundary(input.selected_range.start));
			assert!(input.content.is_char_boundary(input.selected_range.end));
			assert_eq!(input.content(), "你abc");
		});
		visual.update(crate::ui_frame::draw_current_frame);
	}
}
