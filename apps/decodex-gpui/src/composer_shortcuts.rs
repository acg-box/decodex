//! Standard text navigation and deletion; no editor modes or extra UI.
use super::*;

actions!(
	decodex_composer_input,
	[
		WordLeft,
		WordRight,
		SelectWordLeft,
		SelectWordRight,
		LineStart,
		LineEnd,
		SelectLineStart,
		SelectLineEnd,
		SelectDocumentStart,
		SelectDocumentEnd,
		SelectUp,
		SelectDown,
		DeleteWordBackward,
		DeleteWordForward,
		DeleteLineBackward,
		DeleteLineForward
	]
);

pub(super) fn bind_keys(cx: &mut App) {
	cx.bind_keys([
		KeyBinding::new("alt-left", WordLeft, Some("ComposerInput")),
		KeyBinding::new("alt-right", WordRight, Some("ComposerInput")),
		KeyBinding::new("alt-shift-left", SelectWordLeft, Some("ComposerInput")),
		KeyBinding::new("alt-shift-right", SelectWordRight, Some("ComposerInput")),
		KeyBinding::new("cmd-left", LineStart, Some("ComposerInput")),
		KeyBinding::new("cmd-right", LineEnd, Some("ComposerInput")),
		KeyBinding::new("cmd-shift-left", SelectLineStart, Some("ComposerInput")),
		KeyBinding::new("cmd-shift-right", SelectLineEnd, Some("ComposerInput")),
		KeyBinding::new("cmd-up", Home, Some("ComposerInput")),
		KeyBinding::new("cmd-down", End, Some("ComposerInput")),
		KeyBinding::new("cmd-shift-up", SelectDocumentStart, Some("ComposerInput")),
		KeyBinding::new("cmd-shift-down", SelectDocumentEnd, Some("ComposerInput")),
		KeyBinding::new("shift-up", SelectUp, Some("ComposerInput")),
		KeyBinding::new("shift-down", SelectDown, Some("ComposerInput")),
		KeyBinding::new("alt-backspace", DeleteWordBackward, Some("ComposerInput")),
		KeyBinding::new("alt-delete", DeleteWordForward, Some("ComposerInput")),
		KeyBinding::new("cmd-backspace", DeleteLineBackward, Some("ComposerInput")),
		KeyBinding::new("cmd-delete", DeleteLineForward, Some("ComposerInput")),
	]);
}

#[derive(Clone, Copy)]
pub(super) enum Boundary {
	WordStart,
	WordEnd,
	LineStart,
	LineEnd,
	DocumentStart,
	DocumentEnd,
	RowUp,
	RowDown,
}

impl ComposerInput {
	fn boundary(&self, boundary: Boundary) -> usize {
		let cursor = self.cursor_offset();
		match boundary {
			Boundary::WordStart =>
				self.content[..cursor].unicode_word_indices().next_back().map_or(0, |(i, _)| i),
			Boundary::WordEnd => self.content[cursor..]
				.unicode_word_indices()
				.next()
				.map_or(self.content.len(), |(i, word)| cursor + i + word.len()),
			Boundary::DocumentStart => 0,
			Boundary::DocumentEnd => self.content.len(),
			_ =>
				if let Some(lines) = self.last_layout.as_ref() {
					let mut position = text::position_at(lines, cursor);
					match boundary {
						Boundary::LineStart => position.x = px(0.),
						Boundary::LineEnd => position.x = px(1_000_000.),
						Boundary::RowUp => position.y -= px(ui_theme::BODY_LINE_HEIGHT),
						Boundary::RowDown => position.y += px(ui_theme::BODY_LINE_HEIGHT),
						_ => unreachable!(),
					}
					text::index_at(lines, position).min(self.content.len())
				} else {
					match boundary {
						Boundary::LineStart =>
							self.content[..cursor].rfind('\n').map_or(0, |i| i + 1),
						Boundary::LineEnd => self.content[cursor..]
							.find('\n')
							.map_or(self.content.len(), |i| cursor + i),
						_ => cursor,
					}
				},
		}
	}

	pub(super) fn move_boundary(
		&mut self,
		boundary: Boundary,
		select: bool,
		cx: &mut Context<Self>,
	) {
		let target = self.boundary(boundary);
		if select {
			self.select_to(target, cx);
		} else {
			self.move_to(target, cx);
		}
	}

	pub(super) fn delete_boundary(
		&mut self,
		boundary: Boundary,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.selected_range.is_empty() {
			let target = self.boundary(boundary);
			if target == self.cursor_offset() {
				return;
			}
			self.select_to(target, cx);
		}
		self.replace_text_in_range(None, "", window, cx);
	}
}

pub(super) fn bind_actions(
	input: gpui::Stateful<gpui::Div>,
	cx: &mut Context<ComposerInput>,
) -> gpui::Stateful<gpui::Div> {
	input
		.on_action(
			cx.listener(|s, _: &WordLeft, _, cx| s.move_boundary(Boundary::WordStart, false, cx)),
		)
		.on_action(
			cx.listener(|s, _: &WordRight, _, cx| s.move_boundary(Boundary::WordEnd, false, cx)),
		)
		.on_action(cx.listener(|s, _: &SelectWordLeft, _, cx| {
			s.move_boundary(Boundary::WordStart, true, cx)
		}))
		.on_action(
			cx.listener(|s, _: &SelectWordRight, _, cx| {
				s.move_boundary(Boundary::WordEnd, true, cx)
			}),
		)
		.on_action(
			cx.listener(|s, _: &LineStart, _, cx| s.move_boundary(Boundary::LineStart, false, cx)),
		)
		.on_action(
			cx.listener(|s, _: &LineEnd, _, cx| s.move_boundary(Boundary::LineEnd, false, cx)),
		)
		.on_action(cx.listener(|s, _: &SelectLineStart, _, cx| {
			s.move_boundary(Boundary::LineStart, true, cx)
		}))
		.on_action(
			cx.listener(|s, _: &SelectLineEnd, _, cx| s.move_boundary(Boundary::LineEnd, true, cx)),
		)
		.on_action(cx.listener(|s, _: &SelectDocumentStart, _, cx| {
			s.move_boundary(Boundary::DocumentStart, true, cx)
		}))
		.on_action(cx.listener(|s, _: &SelectDocumentEnd, _, cx| {
			s.move_boundary(Boundary::DocumentEnd, true, cx)
		}))
		.on_action(cx.listener(|s, _: &SelectUp, _, cx| s.move_boundary(Boundary::RowUp, true, cx)))
		.on_action(
			cx.listener(|s, _: &SelectDown, _, cx| s.move_boundary(Boundary::RowDown, true, cx)),
		)
		.on_action(cx.listener(|s, _: &DeleteWordBackward, window, cx| {
			s.delete_boundary(Boundary::WordStart, window, cx)
		}))
		.on_action(cx.listener(|s, _: &DeleteWordForward, window, cx| {
			s.delete_boundary(Boundary::WordEnd, window, cx)
		}))
		.on_action(cx.listener(|s, _: &DeleteLineBackward, window, cx| {
			s.delete_boundary(Boundary::LineStart, window, cx)
		}))
		.on_action(cx.listener(|s, _: &DeleteLineForward, window, cx| {
			s.delete_boundary(Boundary::LineEnd, window, cx)
		}))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn editing_keys_preserve_other_lines_and_support_undo(cx: &mut gpui::TestAppContext) {
		cx.update(super::super::bind_keys);
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		visual.simulate_resize(size(px(500.), px(200.)));
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			input.update(cx, |s, cx| s.set_content("first line\nhello, world", cx));
			window.draw(cx).clear();
		});
		visual.simulate_keystrokes("alt-backspace");
		input.read_with(visual, |s, _| assert_eq!(s.content(), "first line\nhello, "));
		visual.simulate_keystrokes("cmd-backspace");
		input.read_with(visual, |s, _| assert_eq!(s.content(), "first line\n"));
		visual.simulate_keystrokes("cmd-z");
		input.read_with(visual, |s, _| assert_eq!(s.content(), "first line\nhello, "));
		visual.simulate_keystrokes("cmd-right cmd-shift-left");
		input.read_with(visual, |s, _| assert_eq!(&s.content[s.selected_range.clone()], "hello, "));
		visual.simulate_keystrokes("alt-backspace");
		input.read_with(visual, |s, _| assert_eq!(s.content(), "first line\n"));
		visual.simulate_keystrokes("cmd-up alt-delete");
		input.read_with(visual, |s, _| assert_eq!(s.content(), " line\n"));
		visual.simulate_keystrokes("cmd-delete");
		input.read_with(visual, |s, _| assert_eq!(s.content(), "\n"));
	}

	#[test]
	fn character_navigation_keeps_composed_graphemes_intact() {
		let text = "你e\u{301}👩‍💻";
		assert_eq!(previous_boundary(text, text.len()), "你e\u{301}".len());
		assert_eq!(next_boundary(text, "你".len()), "你e\u{301}".len());
		assert_eq!(previous_boundary(text, "你e\u{301}".len()), "你".len());
	}
}
