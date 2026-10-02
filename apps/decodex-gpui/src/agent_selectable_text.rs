//! Read-only rich text with native hit testing and clipboard selection.
use std::{ops::Range, path::Path};

use gpui::{App, HighlightStyle, IntoElement, KeyDownEvent, MouseButton, RenderOnce, StyledText};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::shell::agent_surface::{
	ClipboardItem, FocusHandle, InteractiveElement, ParentElement, Role, SharedString,
	StatefulInteractiveElement, Styled, Window, markdown,
};
#[cfg(test)] use crate::shell::agent_surface::{Context, FontWeight};

#[derive(IntoElement)]
pub(super) struct SelectableText {
	pub key: String,
	pub text: String,
	pub highlights: Vec<(Range<usize>, HighlightStyle)>,
	pub links: Vec<(Range<usize>, String)>,
}
impl RenderOnce for SelectableText {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("selection-{}", self.key)),
			cx,
			|_, cx| Selection { anchor: 0, head: 0, dragging: false, focus: cx.focus_handle() },
		);
		let (range, focus) = {
			let s = state.read(cx);

			(s.range(&self.text), s.focus.clone())
		};
		let highlights = selection_highlights(self.text.len(), self.highlights, range);
		let styled = StyledText::new(self.text.clone()).with_highlights(highlights);
		let down_layout = styled.layout().clone();
		let move_layout = down_layout.clone();
		let up_layout = down_layout.clone();
		let down_state = state.clone();
		let move_state = state.clone();
		let up_state = state.clone();
		let key_state = state.clone();
		let down_text = self.text.clone();
		let selector = self.key.clone();

		gpui::div()
			.id(SharedString::from(self.key))
			.debug_selector(move || selector.clone())
			.role(Role::Label)
			.aria_label(self.text.clone())
			.track_focus(&focus)
			.cursor_text()
			.on_mouse_down(MouseButton::Left, move |event, window, cx| {
				let ix = down_layout
					.index_for_position(event.position)
					.unwrap_or_else(|ix| ix)
					.min(down_text.len());

				down_state.update(cx, |s, cx| {
					s.focus.focus(window, cx);

					s.anchor = ix;
					s.head = ix;
					s.dragging = true;

					if event.click_count >= 3 {
						s.anchor = 0;
						s.head = down_text.len();
					} else if event.click_count == 2
						&& let Some((start, word)) = down_text
							.split_word_bound_indices()
							.find(|(start, word)| *start <= ix && ix < start + word.len())
					{
						s.anchor = start;
						s.head = start + word.len();
					}

					cx.notify();
				});

				cx.stop_propagation();
			})
			.on_mouse_move(move |event, _, cx| {
				if move_state.read(cx).dragging && event.pressed_button == Some(MouseButton::Left) {
					let ix = move_layout.index_for_position(event.position).unwrap_or_else(|ix| ix);

					move_state.update(cx, |s, cx| {
						s.head = ix;

						cx.notify();
					});
				}
			})
			.on_mouse_up(MouseButton::Left, move |event, _, cx| {
				let click = {
					let s = up_state.read(cx);

					s.dragging && s.anchor == s.head
				};

				up_state.update(cx, |s, cx| {
					s.dragging = false;

					cx.notify();
				});

				if click
					&& let Ok(ix) = up_layout.index_for_position(event.position)
					&& let Some((_, url)) = self.links.iter().find(|(range, _)| range.contains(&ix))
				{
					open_selection_link(url, cx);
				}
			})
			.on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
				state.update(cx, |s, _| s.dragging = false);
			})
			.on_key_down(move |event: &KeyDownEvent, _, cx| {
				if event.keystroke.modifiers.platform && event.keystroke.key == "c" {
					let s = key_state.read(cx);
					let range = s.range(&self.text);

					if let Some(text) = self.text.get(range).filter(|text| !text.is_empty()) {
						cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
					}

					cx.stop_propagation();
				} else if event.keystroke.modifiers.platform && event.keystroke.key == "a" {
					key_state.update(cx, |s, cx| {
						s.anchor = 0;
						s.head = self.text.len();

						cx.notify();
					});

					cx.stop_propagation();
				}
			})
			.child(styled)
	}
}

struct Selection {
	anchor: usize,
	head: usize,
	dragging: bool,
	focus: FocusHandle,
}
impl Selection {
	fn range(&self, text: &str) -> Range<usize> {
		text.floor_char_boundary(self.anchor.min(self.head).min(text.len()))
			..text.floor_char_boundary(self.anchor.max(self.head).min(text.len()))
	}
}
fn open_selection_link(url: &str, cx: &mut App) {
	if url.starts_with("https://")
		|| url.starts_with("http://")
		|| url.starts_with("codex://threads/")
	{
		cx.open_url(url);
	} else if url.starts_with('/') {
		let path = markdown::without_line_column(url);

		cx.reveal_path(Path::new(path));
	}
}

fn selection_highlights(
	length: usize,
	highlights: Vec<(Range<usize>, HighlightStyle)>,
	selection: Range<usize>,
) -> Vec<(Range<usize>, HighlightStyle)> {
	let mut boundaries = vec![0, length];

	for (range, _) in &highlights {
		boundaries.extend([range.start, range.end]);
	}

	if !selection.is_empty() {
		boundaries.extend([selection.start, selection.end]);
	}

	boundaries.sort_unstable();
	boundaries.dedup();

	boundaries
		.windows(2)
		.map(|ends| {
			let range = ends[0]..ends[1];
			let mut style = highlights
				.iter()
				.find(|(r, _)| r.contains(&range.start))
				.map(|(_, s)| *s)
				.unwrap_or_default();

			if selection.contains(&range.start) {
				style.background_color = Some(gpui::rgba(0x788dff66).into());
			}

			(range, style)
		})
		.collect()
}
#[cfg(test)]
mod tests {

	use crate::shell::agent_surface::selectable_text::{
		self, ClipboardItem, Context, FontWeight, IntoElement, SelectableText, Window,
	};

	struct Preview {
		text: String,
	}
	impl gpui::Render for Preview {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			SelectableText {
				key: "changing-selection".into(),
				text: self.text.clone(),
				highlights: Vec::new(),
				links: Vec::new(),
			}
		}
	}

	#[test]
	fn selection_splits_styled_runs_without_overlap() {
		let bold =
			gpui::HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() };
		let runs = selectable_text::selection_highlights(
			12,
			vec![(0..6, bold), (6..12, Default::default())],
			3..9,
		);

		assert_eq!(
			runs.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(),
			vec![0..3, 3..6, 6..9, 9..12]
		);
		assert_eq!(runs[1].1.font_weight, Some(FontWeight::BOLD));
		assert!(runs[1].1.background_color.is_some());
		assert!(runs[3].1.background_color.is_none());
	}

	#[gpui::test]
	fn copy_uses_the_visible_selection_after_unicode_text_changes(cx: &mut gpui::TestAppContext) {
		let (preview, visual) = cx.add_window_view(|_, _| Preview { text: "abcd".into() });

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let bounds = visual.debug_bounds("changing-selection").expect("text");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.simulate_keystrokes("cmd-a");

		preview.update(visual, |s, cx| {
			s.text = "中文".into();

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
			cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into()));
		});
		visual.simulate_keystrokes("cmd-c");
		visual.update(|_, cx| {
			assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some("中".into()));
		});
	}
}
