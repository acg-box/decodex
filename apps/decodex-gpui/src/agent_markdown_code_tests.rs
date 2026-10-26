//! Wide code must not resize the message, wrap lines, or move its copy control.
use gpui::{
	Context, IntoElement, ParentElement as _, Render, ScrollDelta, ScrollEvent, Styled as _,
	TestAppContext, Window,
};

use crate::shell::agent_surface::markdown::{self, Kind, NodeContent};

struct Preview {
	source: String,
}
impl Render for Preview {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		gpui::div().w_full().min_w_0().child(markdown::render(&self.source, "code-preview"))
	}
}

#[test]
fn code_fences_keep_language_separate_from_metadata_and_source() {
	for (source, expected) in [
		("```rust title=example\nlet x = 1;\n```", Some("rust")),
		("~~~python,example\nprint('book')\n~~~", Some("python")),
		("```\nplain\n```", None),
		("    indented\n", None),
		("```mermaid\ngraph LR; A -->", Some("mermaid")),
	] {
		let nodes = markdown::parse(source);
		let NodeContent::Block(Kind::Code { language }, _) = &nodes[0].content else {
			panic!("code block: {source:?}");
		};

		assert_eq!(language.as_deref(), expected);
	}
}

#[gpui::test]
fn wide_code_scrolls_without_wrapping_and_keeps_copy_reachable(cx: &mut TestAppContext) {
	let code =
		format!("  const books = [{}];  \r\n\tconsole.log(books);\r\n", "\"茶\", ".repeat(32));
	let (_, visual) = cx.add_window_view(|_, _| Preview {
		source: format!("Before.\n\n```javascript\r\n{code}```\n\nAfter."),
	});
	let mut previous_size = None;

	for width in [800., 220., 600.] {
		visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(500.)));
		visual.update(crate::ui_frame::draw_current_frame);

		let viewport = visual.debug_bounds("code-viewport-code-preview-1").expect("code viewport");

		assert!(viewport.right() <= gpui::px(width));

		let text = visual.debug_bounds("code-preview-1").expect("code text");

		if let Some(size) = previous_size {
			assert_eq!(text.size, size, "code must not wrap when the message resizes");
		}

		previous_size = Some(text.size);

		let button = visual.debug_bounds("copy-code-code-preview-1").expect("code copy");

		assert!(button.right() <= viewport.right());

		visual.simulate_event(ScrollEvent {
			position: viewport.center(),
			delta: ScrollDelta::Pixels(gpui::point(gpui::px(-60.), gpui::px(0.))),
			..Default::default()
		});
		visual.update(crate::ui_frame::draw_current_frame);

		let moved = visual.debug_bounds("code-preview-1").expect("scrolled code");

		assert!(moved.origin.x < text.origin.x, "long code must scroll horizontally");
		assert_eq!(visual.debug_bounds("copy-code-code-preview-1"), Some(button));

		visual.simulate_click(button.center(), Default::default());
		visual.update(|_, cx| {
			assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some(code.clone()));
		});
	}
}
