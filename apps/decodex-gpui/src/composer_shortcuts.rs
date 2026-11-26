//! Standard text navigation and deletion; no editor modes or extra UI.
use gpui::{Div, InteractiveElement as _, Stateful};

use crate::composer_input::{App, ComposerInput, Context, KeyBinding, text, word_boundary};

#[derive(Clone, Copy, Debug, PartialEq, gpui::Action)]
#[action(namespace = decodex_composer_input, no_json)]
enum Navigation {
	Move(Boundary),
	Select(Boundary),
	Delete(Boundary),
	Vertical(f32, bool),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Boundary {
	WordStart,
	WordEnd,
	LineStart,
	LineEnd,
	ParagraphStart,
	ParagraphEnd,
	DocumentStart,
	DocumentEnd,
}

impl ComposerInput {
	fn boundary(&self, boundary: Boundary) -> (usize, bool) {
		let cursor = self.cursor_offset();

		let offset = match boundary {
			Boundary::WordStart if self.secret => 0,
			Boundary::WordEnd if self.secret => self.content.len(),
			Boundary::WordStart => word_boundary(&self.content, cursor, false),
			Boundary::WordEnd => word_boundary(&self.content, cursor, true),
			Boundary::DocumentStart => 0,
			Boundary::DocumentEnd => self.content.len(),
			Boundary::ParagraphStart => {
				let before = &self.content[..cursor];
				before.strip_suffix('\n').unwrap_or(before).rfind('\n').map_or(0, |i| i + 1)
			},
			Boundary::ParagraphEnd => {
				let start = cursor + usize::from(self.content[cursor..].starts_with('\n'));
				self.content[start..].find('\n').map_or(self.content.len(), |i| start + i)
			},
			Boundary::LineStart | Boundary::LineEnd => {
				let row = self.last_layout.as_deref().and_then(|lines| {
					text::row_range_at(
						lines,
						text::caret_position(lines, cursor, self.caret_after_wrap),
					)
				});
				if matches!(boundary, Boundary::LineStart) {
					let start = row.map_or_else(
						|| self.content[..cursor].rfind('\n').map_or(0, |i| i + 1),
						|row| row.start,
					);
					return (start, true);
				}
				row.map_or_else(
					|| self.content[cursor..].find('\n').map_or(self.content.len(), |i| cursor + i),
					|row| row.end,
				)
			},
		};

		(offset, false)
	}
}

pub(crate) fn shortcut_motion(action: &dyn gpui::Action) -> Option<String> {
	let motion = action.as_any().downcast_ref::<Navigation>()?;
	Some(match motion {
		Navigation::Move(b) => format!("MoveTo{b:?}"),
		Navigation::Select(b) => format!("SelectTo{b:?}"),
		Navigation::Delete(b) => format!("DeleteTo{b:?}"),
		Navigation::Vertical(d, select) => format!(
			"{}{}",
			if *select { "Select" } else { "Move" },
			if *d < 0. { "Up" } else { "Down" }
		),
	})
}

pub(super) fn bind_keys(cx: &mut App) {
	use Boundary::*;
	use Navigation::*;

	cx.bind_keys(
		[
			("alt-left", Move(WordStart)),
			("alt-right", Move(WordEnd)),
			("alt-shift-left", Select(WordStart)),
			("alt-shift-right", Select(WordEnd)),
			("cmd-left", Move(LineStart)),
			("cmd-right", Move(LineEnd)),
			("cmd-shift-left", Select(LineStart)),
			("cmd-shift-right", Select(LineEnd)),
			("alt-up", Move(ParagraphStart)),
			("alt-down", Move(ParagraphEnd)),
			("alt-shift-up", Select(ParagraphStart)),
			("alt-shift-down", Select(ParagraphEnd)),
			("cmd-up", Move(DocumentStart)),
			("cmd-down", Move(DocumentEnd)),
			("cmd-shift-up", Select(DocumentStart)),
			("cmd-shift-down", Select(DocumentEnd)),
			("home", Move(DocumentStart)),
			("end", Move(DocumentEnd)),
			("shift-home", Select(DocumentStart)),
			("shift-end", Select(DocumentEnd)),
			("up", Vertical(-1.0, false)),
			("down", Vertical(1.0, false)),
			("shift-up", Vertical(-1.0, true)),
			("shift-down", Vertical(1.0, true)),
			("alt-backspace", Delete(WordStart)),
			("alt-delete", Delete(WordEnd)),
			("cmd-backspace", Delete(LineStart)),
			("cmd-delete", Delete(LineEnd)),
		]
		.into_iter()
		.map(|(key, motion)| KeyBinding::new(key, motion, Some("ComposerInput"))),
	);
}

pub(super) fn bind_actions(input: Stateful<Div>, cx: &mut Context<ComposerInput>) -> Stateful<Div> {
	input.on_action(cx.listener(|input, action: &Navigation, _, cx| match *action {
		Navigation::Move(boundary) => input.apply_caret(input.boundary(boundary), false, cx),
		Navigation::Select(boundary) => input.apply_caret(input.boundary(boundary), true, cx),
		Navigation::Delete(boundary) => {
			input.delete_to(input.boundary(boundary).0, None, cx);
		},
		Navigation::Vertical(direction, extend) => input.vertical(direction, extend, cx),
	}))
}

#[cfg(test)]
mod tests {
	use gpui::Focusable as _;

	use crate::composer_input::{self, shortcuts::ComposerInput};

	#[gpui::test]
	fn editing_keys_preserve_other_lines_and_support_undo(cx: &mut gpui::TestAppContext) {
		cx.update(super::super::bind_keys);

		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));

		visual.simulate_resize(gpui::size(gpui::px(500.), gpui::px(200.)));
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			input.update(cx, |s, cx| s.set_content("first line\nhello, world", cx));
			window.draw(cx).clear(cx);
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

	#[gpui::test]
	fn caret_stays_on_its_visual_row_and_retains_column(cx: &mut gpui::TestAppContext) {
		cx.update(super::super::bind_keys);
		let (input, visual) = cx.add_window_view(|_, cx| ComposerInput::new(0, cx));
		visual.simulate_resize(gpui::size(gpui::px(205.), gpui::px(300.)));
		let draft = "这是需要自动换行的中文草稿，Wrap selection。";
		input.update(visual, |input, cx| input.set_content(draft, cx));
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			crate::ui_frame::draw_current_frame(window, cx);
		});
		visual.simulate_keystrokes("cmd-left cmd-shift-right");
		input.read_with(visual, |input, _| {
			let line = &input.last_layout.as_ref().unwrap()[0];
			let [boundary] = line.wrap_boundaries() else { panic!("two visual rows") };
			let split = line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index;
			assert_eq!(input.selected_range, split..draft.len());
		});

		input.update(visual, |input, cx| input.set_content("mmmmmm\nm\nmmmmmm", cx));
		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_keystrokes("left left up up");
		input.read_with(visual, |input, _| assert_eq!(input.selected_range, 4..4));
	}

	#[test]
	fn character_navigation_keeps_composed_graphemes_intact() {
		let text = "你e\u{301}👩‍💻";

		assert_eq!(composer_input::previous_boundary(text, text.len()), "你e\u{301}".len());
		assert_eq!(composer_input::next_boundary(text, "你".len()), "你e\u{301}".len());
		assert_eq!(composer_input::previous_boundary(text, "你e\u{301}".len()), "你".len());
	}
}
