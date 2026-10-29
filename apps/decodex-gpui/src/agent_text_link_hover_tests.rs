//! Link hover uses the same shaped text and destination rules as a click.
use gpui::{CursorStyle, StyledText, TestAppContext, TextLayout, VisualTestContext, point, px};

use super::link_cursor;

fn draw_text(cx: &mut VisualTestContext, text: &str, width: f32, top: f32) -> TextLayout {
	let text = StyledText::new(text.to_owned());
	let layout = text.layout().clone();
	cx.draw(point(px(20.), px(top)), gpui::size(px(width), px(200.)), |_, _| text);
	layout
}

#[gpui::test]
fn link_pointer_matches_actionable_glyphs_and_excludes_whitespace(cx: &mut TestAppContext) {
	let visual = cx.add_empty_window();
	let text = "plain 界 link after";
	let start = text.find('界').expect("link start");
	let end = text.find(" after").expect("link end");
	let layout = draw_text(visual, text, 300., 20.);
	let over_link = layout.position_for_index(start).expect("link glyph") + point(px(1.), px(3.));

	for destination in [
		"https://example.com/文档_(one)",
		"http://example.com",
		"codex://threads/native-id",
		"/tmp/example.rs:12:3",
	] {
		let links = vec![(start..end, destination.to_owned())];
		assert_eq!(link_cursor(&layout, &links, over_link), CursorStyle::PointingHand);
		for at in [
			layout.position_for_index(0).expect("plain glyph") + point(px(1.), px(3.)),
			layout.position_for_index(end).expect("after link") + point(px(1.), px(3.)),
			point(layout.bounds().right() + px(5.), over_link.y),
			point(over_link.x, layout.bounds().bottom() + px(5.)),
		] {
			assert_eq!(link_cursor(&layout, &links, at), CursorStyle::IBeam);
		}
	}
	for destination in ["relative.md", "javascript:alert(1)", "mailto:reader@example.com"] {
		let links = vec![(start..end, destination.to_owned())];
		assert_eq!(link_cursor(&layout, &links, over_link), CursorStyle::IBeam);
	}
}

#[gpui::test]
fn link_pointer_recomputes_wrapped_targets_after_reflow_and_movement(cx: &mut TestAppContext) {
	let visual = cx.add_empty_window();
	let text = "before wide 界 wrapped label after";
	let start = text.find("wide").expect("link start");
	let end = text.find(" after").expect("link end");
	let links = vec![(start..end, "https://example.com".to_owned())];
	let layout = draw_text(visual, text, 100., 20.);
	let first = layout.position_for_index(start).expect("first line");
	let last = layout.position_for_index(end - 1).expect("last line");
	assert!(last.y > first.y, "the link must wrap");
	for (index, character) in text.char_indices().filter(|(index, _)| (start..end).contains(index))
	{
		if !character.is_whitespace() {
			// At a wrap boundary GPUI returns the previous line's end for the caret.
			// Sample inside the glyph by moving back from its trailing caret instead.
			let at =
				layout.position_for_index(index + character.len_utf8()).expect("wrapped glyph")
					+ point(px(-1.), px(3.));
			assert_eq!(
				link_cursor(&layout, &links, at),
				CursorStyle::PointingHand,
				"{character:?} at byte {index}, position {at:?}, hit {:?}",
				layout.index_for_position(at)
			);
		}
	}

	let old_target = last + point(px(1.), px(3.));
	let layout = draw_text(visual, text, 400., 20.);
	assert_eq!(link_cursor(&layout, &links, old_target), CursorStyle::IBeam);
	let at = layout.position_for_index(start).expect("reflowed link") + point(px(1.), px(3.));
	assert_eq!(link_cursor(&layout, &links, at), CursorStyle::PointingHand);

	let layout = draw_text(visual, text, 400., 120.);
	assert_eq!(link_cursor(&layout, &links, at), CursorStyle::IBeam);
	let at = layout.position_for_index(start).expect("moved link") + point(px(1.), px(3.));
	assert_eq!(link_cursor(&layout, &links, at), CursorStyle::PointingHand);
}
