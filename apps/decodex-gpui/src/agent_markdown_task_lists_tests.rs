//! Preserve task state when the Markdown parser consumes markers before block content.
use super::{HighlightStyle, Inline, append_inline, parse};

#[test]
fn task_markers_survive_block_content_without_duplicates() {
	for (source, expected) in [
		("- [x] # title\n", "☑ title"),
		("- [ ] \n  - child\n", "☐ child"),
		("- [x] > quote\n", "☑ quote"),
		("9. [X] # title\n", "☑ title"),
		("- [x] **done**\n", "☑ done"),
		("- [ ]\n- [x]\n", "[ ][x]"),
		("- \\[x] literal\n", "[x] literal"),
		("-     [x] code, not a task\n", "[x] code, not a task\n"),
	] {
		let mut rendered = Inline::default();
		append_inline(&parse(source), HighlightStyle::default(), None, &mut rendered);
		assert_eq!(rendered.text, expected, "{source:?}");
	}
}
