//! Read-only rich text with native hit testing and clipboard selection.
use std::{cell::OnceCell, ops::Range, path::Path, rc::Rc};

#[path = "agent_text_selection.rs"] mod selection;
pub(super) use selection::Document;

use gpui::{App, Element, HighlightStyle, IntoElement, MouseButton, RenderOnce, StyledText};
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
impl SelectableText {
	pub(super) fn plain(key: String, text: String) -> Self {
		Self { key, text, highlights: Vec::new(), links: Vec::new() }
	}
}
impl RenderOnce for SelectableText {
	fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
		let key = self.key.clone();
		let mut document = Document::default();
		let child = document.push(self);
		document.finish(&key, gpui::div().child(child))
	}
}

#[derive(IntoElement)]
struct TextBlock {
	text: SelectableText,
	range: Range<usize>,
	scope: Rc<OnceCell<selection::Scope>>,
}
impl RenderOnce for TextBlock {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let scope = self.scope.get().expect("selection scope");
		let selected = scope.state.read(cx).range(&scope.text);
		let local = selected.start.max(self.range.start).min(self.range.end) - self.range.start
			..selected.end.max(self.range.start).min(self.range.end) - self.range.start;
		let highlights = selection_highlights(self.text.text.len(), self.text.highlights, local);
		let batch =
			highlights.iter().all(|(_, s)| s.underline.is_none() && s.strikethrough.is_none())
				&& window.text_style().underline.is_none()
				&& window.text_style().strikethrough.is_none();
		let styled = StyledText::new(self.text.text.clone()).with_highlights(highlights);
		let layout = styled.layout().clone();
		let down_state = scope.state.clone();
		let up_state = scope.state.clone();
		let right_scope = self.scope.clone();
		let right_layout = layout.clone();
		let right_text = self.text.text.clone();
		let down_layout = layout.clone();
		let start = self.range.start;
		let end = self.range.end;
		let text = self.text.text;
		let selector = self.text.key.clone();
		gpui::div()
			.id(SharedString::from(self.text.key))
			.debug_selector(move || selector.clone())
			.role(Role::Label)
			.aria_label(text.clone())
			.cursor_text()
			.on_mouse_down(MouseButton::Right, move |event, window, cx| {
				let scope = right_scope.get().expect("selection scope");
				let ix = right_layout
					.index_for_position(event.position)
					.unwrap_or_else(|ix| ix)
					.min(right_text.len());
				if !scope.state.read(cx).range(&scope.text).contains(&(start + ix)) {
					scope.state.update(cx, |s, cx| {
						s.focus.focus(window, cx);
						if let Some((offset, word)) = right_text
							.split_word_bound_indices()
							.find(|(offset, word)| *offset <= ix && ix < offset + word.len())
						{
							s.anchor = start + offset;
							s.head = s.anchor + word.len();
						}
						cx.notify();
					});
				}
				scope.context_menu(window, cx);
				cx.stop_propagation();
			})
			.on_mouse_down(MouseButton::Left, move |event, window, cx| {
				let ix = down_layout
					.index_for_position(event.position)
					.unwrap_or_else(|ix| ix)
					.min(text.len());
				down_state.update(cx, |s, cx| {
					s.focus.focus(window, cx);
					if !event.modifiers.shift {
						s.anchor = start + ix;
					}
					s.head = start + ix;
					s.dragging = true;
					if event.click_count >= 3 {
						s.anchor = start;
						s.head = end;
					} else if event.click_count == 2
						&& let Some((offset, word)) = text
							.split_word_bound_indices()
							.find(|(offset, word)| *offset <= ix && ix < offset + word.len())
					{
						s.anchor = start + offset;
						s.head = start + offset + word.len();
					}
					cx.notify();
				});
				cx.stop_propagation();
			})
			.on_mouse_up(MouseButton::Left, move |event, _, cx| {
				let s = up_state.read(cx);
				if s.dragging
					&& s.anchor == s.head
					&& let Ok(ix) = layout.index_for_position(event.position)
					&& s.anchor == start + ix
					&& let Some((_, url)) =
						self.text.links.iter().find(|(range, _)| range.contains(&ix))
				{
					open_selection_link(url, cx);
				}
			})
			.child(TextLayer { styled, batch, range: self.range, scope: self.scope })
	}
}

/// A paragraph has one foreground order. Keep selection backgrounds underneath
/// it instead of inserting every glyph into the scene's overlap tree each frame.
/// Decorated text keeps GPUI's original interleaving of glyphs and decorations.
struct TextLayer {
	styled: StyledText,
	batch: bool,
	range: Range<usize>,
	scope: Rc<OnceCell<selection::Scope>>,
}
impl IntoElement for TextLayer {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}
impl Element for TextLayer {
	type PrepaintState = ();
	type RequestLayoutState = ();

	fn id(&self) -> Option<gpui::ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		id: Option<&gpui::GlobalElementId>,
		inspector: Option<&gpui::InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (gpui::LayoutId, ()) {
		self.styled.request_layout(id, inspector, window, cx)
	}

	fn prepaint(
		&mut self,
		id: Option<&gpui::GlobalElementId>,
		inspector: Option<&gpui::InspectorElementId>,
		bounds: gpui::Bounds<gpui::Pixels>,
		state: &mut (),
		window: &mut Window,
		cx: &mut App,
	) {
		self.styled.prepaint(id, inspector, bounds, state, window, cx);
		self.scope
			.get()
			.expect("selection scope")
			.layouts
			.borrow_mut()
			.push((self.range.clone(), self.styled.layout().clone()));
	}

	fn paint(
		&mut self,
		_: Option<&gpui::GlobalElementId>,
		_: Option<&gpui::InspectorElementId>,
		bounds: gpui::Bounds<gpui::Pixels>,
		_: &mut (),
		_: &mut (),
		window: &mut Window,
		cx: &mut App,
	) {
		if self.batch {
			self.styled.layout().paint_background(window, cx).expect("text background");
			window.paint_layer(bounds, |window| {
				self.styled.layout().paint_foreground(window, cx).expect("text foreground");
			});
		} else {
			self.styled.paint(None, None, bounds, &mut (), &mut (), window, cx);
		}
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
	use gpui::prelude::{ParentElement as _, Styled as _};

	use crate::shell::agent_surface::selectable_text::{
		self, ClipboardItem, Context, FontWeight, IntoElement, SelectableText, Window,
	};

	struct Preview {
		text: String,
	}
	impl gpui::Render for Preview {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			gpui::div().w(gpui::px(250.)).child(SelectableText {
				key: "changing-selection".into(),
				text: self.text.clone(),
				highlights: Vec::new(),
				links: Vec::new(),
			})
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
	fn dragging_past_the_text_keeps_extending_the_selection(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, _| Preview { text: "A calm reading day.".into() });
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let bounds = visual.debug_bounds("changing-selection").expect("text");
		let start = gpui::point(bounds.left() + gpui::px(1.), bounds.top() + gpui::px(8.));
		let end = gpui::point(bounds.right() + gpui::px(24.), bounds.bottom() + gpui::px(24.));
		visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
		visual.simulate_mouse_move(end, gpui::MouseButton::Left, Default::default());
		visual.simulate_mouse_up(end, gpui::MouseButton::Left, Default::default());
		visual.simulate_keystrokes("cmd-c");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
				Some("A calm reading day.")
			);
		});
	}

	#[gpui::test]
	fn copy_uses_the_visible_selection_after_unicode_text_changes(cx: &mut gpui::TestAppContext) {
		let (preview, visual) = cx.add_window_view(|_, _| Preview { text: "abcd".into() });

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		let bounds = visual.debug_bounds("changing-selection").expect("text");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.simulate_keystrokes("cmd-a");

		preview.update(visual, |s, cx| {
			s.text = "中文".into();

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
			cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into()));
		});
		visual.simulate_keystrokes("cmd-c");
		visual.update(|_, cx| {
			assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some("中".into()));
		});
	}
}
