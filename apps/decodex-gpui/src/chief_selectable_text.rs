//! Read-only rich text with native hit testing and clipboard selection.
use super::*;
use gpui::{App, IntoElement, MouseButton, RenderOnce, StyledText};
use std::ops::Range;

#[derive(IntoElement)]
pub(super) struct SelectableText {
	pub key: String,
	pub text: String,
	pub highlights: Vec<(Range<usize>, gpui::HighlightStyle)>,
	pub links: Vec<(Range<usize>, String)>,
}
struct Selection {
	anchor: usize,
	head: usize,
	dragging: bool,
	focus: FocusHandle,
}
impl RenderOnce for SelectableText {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("selection-{}", self.key)),
			cx,
			|_, cx| Selection { anchor: 0, head: 0, dragging: false, focus: cx.focus_handle() },
		);
		let (anchor, head, focus) = {
			let s = state.read(cx);
			(s.anchor, s.head, s.focus.clone())
		};
		let range = self.text.floor_char_boundary(anchor.min(head).min(self.text.len()))
			..self.text.floor_char_boundary(anchor.max(head).min(self.text.len()));
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
		div()
			.id(SharedString::from(self.key))
			.debug_selector(move || selector.clone())
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
					} else if event.click_count == 2 {
						use unicode_segmentation::UnicodeSegmentation as _;
						if let Some((start, word)) = down_text
							.split_word_bound_indices()
							.find(|(start, word)| *start <= ix && ix < start + word.len())
						{
							s.anchor = start;
							s.head = start + word.len();
						}
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
					if url.starts_with("https://")
						|| url.starts_with("http://")
						|| url.starts_with("codex://threads/")
					{
						cx.open_url(url);
					} else if url.starts_with('/') {
						let path = url
							.rsplit_once(':')
							.filter(|(_, line)| line.parse::<u32>().is_ok())
							.map_or(url.as_str(), |(path, _)| path);
						cx.reveal_path(std::path::Path::new(path));
					}
				}
			})
			.on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
				state.update(cx, |s, _| s.dragging = false);
			})
			.on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
				if event.keystroke.modifiers.platform && event.keystroke.key == "c" {
					let s = key_state.read(cx);
					let range = s.anchor.min(s.head)..s.anchor.max(s.head).min(self.text.len());
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

fn selection_highlights(
	length: usize,
	highlights: Vec<(Range<usize>, gpui::HighlightStyle)>,
	selection: Range<usize>,
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
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
				style.background_color = Some(rgba(0x788dff66).into());
			}
			(range, style)
		})
		.collect()
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn selection_splits_styled_runs_without_overlap() {
		let bold =
			gpui::HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() };
		let runs = selection_highlights(12, vec![(0..6, bold), (6..12, Default::default())], 3..9);
		assert_eq!(
			runs.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(),
			vec![0..3, 3..6, 6..9, 9..12]
		);
		assert_eq!(runs[1].1.font_weight, Some(FontWeight::BOLD));
		assert!(runs[1].1.background_color.is_some());
		assert!(runs[3].1.background_color.is_none());
	}
}
