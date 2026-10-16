//! Select what the reader sees across rich-text block boundaries.
use gpui::{IntoElement, ParentElement as _, Styled as _};

#[gpui::test]
fn whole_blocks_keep_markers_after_another_paragraph(cx: &mut gpui::TestAppContext) {
	for (source, selector, expected) in [
		("Before.\n\n## Heading\n\nAfter.", "selection-preview-1", "## Heading"),
		("Before.\n\n- Book\n\nAfter.", "selection-preview-1-0-text-0", "- Book"),
		("Before.\n\n> Quote\n\nAfter.", "selection-preview-1-0", "> Quote"),
	] {
		let (_, visual) =
			cx.add_window_view(|_, _| Preview { text: source.into(), process: false });
		draw(visual);
		let bounds = visual.debug_bounds(selector).expect("marked block");
		let at = gpui::point(bounds.left() + gpui::px(2.), bounds.top() + gpui::px(8.));
		visual.simulate_mouse_down(at, gpui::MouseButton::Right, Default::default());
		visual.simulate_mouse_up(at, gpui::MouseButton::Right, Default::default());
		visual.simulate_keystrokes("cmd-c");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
				Some(expected)
			)
		});
	}
}

struct Preview {
	text: String,
	process: bool,
}
impl gpui::Render for Preview {
	fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
		gpui::div().w(gpui::px(400.)).child(if self.process {
			super::render_process(&self.text, "selection-preview")
		} else {
			super::render(&self.text, "selection-preview")
		})
	}
}

fn draw(cx: &mut gpui::VisualTestContext) {
	cx.update(|window, cx| {
		window.resize(gpui::size(gpui::px(700.), gpui::px(600.)));
		window.draw(cx).clear(cx);
	});
}

fn copy_drag(
	cx: &mut gpui::VisualTestContext,
	first: &'static str,
	last: &'static str,
	reverse: bool,
) -> String {
	let first = cx.debug_bounds(first).expect("first text");
	let last = cx.debug_bounds(last).expect("last text");
	let start = gpui::point(first.left() + gpui::px(1.), first.top() + gpui::px(8.));
	let end = gpui::point(last.right() + gpui::px(20.), last.bottom() + gpui::px(8.));
	let (start, end) = if reverse {
		(end - gpui::point(gpui::px(21.), gpui::px(9.)), start)
	} else {
		(start, end)
	};
	cx.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
	cx.simulate_mouse_move(end, gpui::MouseButton::Left, Default::default());
	cx.simulate_mouse_up(end, gpui::MouseButton::Left, Default::default());
	cx.simulate_keystrokes("cmd-c");
	cx.update(|_, cx| {
		cx.read_from_clipboard().and_then(|item| item.text()).expect("copied selection")
	})
}

#[gpui::test]
fn one_drag_selects_multiple_paragraphs_in_both_directions(cx: &mut gpui::TestAppContext) {
	let (preview, visual) = cx.add_window_view(|_, _| Preview {
		text: "First **paragraph**.\n\nSecond 中文 paragraph.\n\nThird paragraph.".into(),
		process: false,
	});
	for process in [false, true] {
		preview.update(visual, |s, cx| {
			s.process = process;
			cx.notify();
		});
		draw(visual);
		for reverse in [false, true] {
			assert_eq!(
				copy_drag(visual, "selection-preview-0", "selection-preview-2", reverse),
				"First **paragraph**.\n\nSecond 中文 paragraph.\n\nThird paragraph."
			);
		}
	}
}

#[gpui::test]
fn selection_crosses_lists_and_code_without_copying_controls(cx: &mut gpui::TestAppContext) {
	let (_, visual) = cx.add_window_view(|_, _| Preview {
		text: "Before.\n\n- First item\n- Second item\n\n```rs\nlet book = \"茶\";\n```\n\nAfter."
			.into(),
		process: false,
	});
	draw(visual);
	assert_eq!(
		copy_drag(visual, "selection-preview-0", "selection-preview-3", false),
		"Before.\n\n- First item\n- Second item\n\n```rs\nlet book = \"茶\";\n```\n\nAfter."
	);
}

#[gpui::test]
fn selection_preserves_table_cells_and_quote_paragraphs(cx: &mut gpui::TestAppContext) {
	let (_, visual) = cx.add_window_view(|_, _| Preview {
		text: "> First quote.\n>\n> Second quote.\n\n| Book | Pages |\n|---|---|\n| Dune | 100 |\n\nDone.".into(),
		process: false,
	});
	draw(visual);
	assert_eq!(
		copy_drag(visual, "selection-preview-0-0", "selection-preview-2", false),
		"> First quote.\n>\n> Second quote.\n\n| Book | Pages |\n|---|---|\n| Dune | 100 |\n\nDone."
	);
}

#[gpui::test]
fn shared_selection_survives_appends_and_stops_at_mouse_up(cx: &mut gpui::TestAppContext) {
	let (preview, visual) =
		cx.add_window_view(|_, _| Preview { text: "First.\n\nSecond.".into(), process: false });
	draw(visual);
	assert_eq!(
		copy_drag(visual, "selection-preview-0", "selection-preview-1", false),
		"First.\n\nSecond."
	);
	preview.update(visual, |s, cx| {
		s.text.push_str("\n\nThird.");
		cx.notify();
	});
	draw(visual);
	let third = visual.debug_bounds("selection-preview-2").unwrap();
	visual.simulate_mouse_move(third.center(), gpui::MouseButton::Left, Default::default());
	visual.simulate_keystrokes("cmd-c");
	visual.update(|_, cx| {
		assert_eq!(
			cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
			Some("First.\n\nSecond.")
		)
	});
	visual.simulate_keystrokes("cmd-a cmd-c");
	visual.update(|_, cx| {
		assert_eq!(
			cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
			Some("First.\n\nSecond.\n\nThird.")
		)
	});
	preview.update(visual, |s, cx| {
		s.text = "新段落。\n\n茶。".into();
		cx.notify();
	});
	draw(visual);
	visual.simulate_keystrokes("cmd-c");
	visual.update(|_, cx| {
		assert_eq!(
			cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
			Some("新段落。\n\n茶。")
		)
	});
}

#[gpui::test]
fn links_still_open_on_click_but_never_during_cross_paragraph_selection(
	cx: &mut gpui::TestAppContext,
) {
	let (_, visual) = cx.add_window_view(|_, _| Preview {
		text: "[Read more](https://example.com/books).\n\nContinue reading.".into(),
		process: false,
	});
	draw(visual);
	assert_eq!(
		copy_drag(visual, "selection-preview-0", "selection-preview-1", false),
		"[Read more](https://example.com/books).\n\nContinue reading."
	);
	assert!(visual.opened_url().is_none());
	let first = visual.debug_bounds("selection-preview-0").unwrap();
	visual.simulate_click(
		gpui::point(first.left() + gpui::px(2.), first.top() + gpui::px(8.)),
		Default::default(),
	);
	assert_eq!(visual.opened_url().as_deref(), Some("https://example.com/books"));
}

#[gpui::test]
fn clicking_blank_space_clears_the_whole_selection(cx: &mut gpui::TestAppContext) {
	let (_, visual) =
		cx.add_window_view(|_, _| Preview { text: "First.\n\nSecond.".into(), process: false });
	draw(visual);
	assert_eq!(
		copy_drag(visual, "selection-preview-0", "selection-preview-1", false),
		"First.\n\nSecond."
	);
	visual
		.update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("sentinel".into())));
	visual.simulate_click(gpui::point(gpui::px(600.), gpui::px(500.)), Default::default());
	visual.simulate_keystrokes("cmd-c");
	visual.update(|_, cx| {
		assert_eq!(
			cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
			Some("sentinel")
		)
	});
}

#[gpui::test]
fn a_diagram_shares_selection_with_the_surrounding_paragraphs(cx: &mut gpui::TestAppContext) {
	let (_, visual) = cx.add_window_view(|_, _| Preview {
		text: "Before.\n\n```mermaid\nflowchart LR; A --> B\n```\n\nAfter.".into(),
		process: false,
	});
	draw(visual);
	let selected = copy_drag(visual, "selection-preview-0", "selection-preview-2", false);
	assert!(selected.starts_with("Before.\n\n"));
	assert!(selected.contains('A') && selected.contains('B'));
	assert!(selected.ends_with("\n\nAfter."));
	assert_eq!(selected, "Before.\n\n```mermaid\nflowchart LR; A --> B\n```\n\nAfter.");
}

#[gpui::test]
fn selected_words_copy_their_original_markdown_and_right_click_keeps_selection(
	cx: &mut gpui::TestAppContext,
) {
	for (source, expected) in [
		("**Bold**\n\nAfter.", "**Bold**"),
		("[Read](https://example.com/books)\n\nAfter.", "[Read](https://example.com/books)"),
		("`code`\n\nAfter.", "`code`"),
		("&amp;\n\nAfter.", "&amp;"),
		("**Bold**\r\n\r\nAfter.\r\n", "**Bold**"),
	] {
		let (_, visual) =
			cx.add_window_view(|_, _| Preview { text: source.into(), process: false });
		draw(visual);
		let bounds = visual.debug_bounds("selection-preview-0").unwrap();
		let at = gpui::point(bounds.left() + gpui::px(2.), bounds.top() + gpui::px(8.));
		visual.simulate_mouse_down(at, gpui::MouseButton::Right, Default::default());
		visual.simulate_mouse_up(at, gpui::MouseButton::Right, Default::default());
		visual.simulate_keystrokes("cmd-c");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
				Some(expected)
			)
		});
		let all = copy_drag(visual, "selection-preview-0", "selection-preview-1", false);
		assert_eq!(all, source);
		visual.simulate_mouse_down(at, gpui::MouseButton::Right, Default::default());
		visual.simulate_mouse_up(at, gpui::MouseButton::Right, Default::default());
		visual.simulate_keystrokes("cmd-c");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
				Some(source)
			)
		});
	}
}
