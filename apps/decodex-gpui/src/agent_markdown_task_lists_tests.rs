//! Preserve native task markers, literal paragraph text, escapes, and code blocks.
use crate::shell::agent_surface::markdown::{self, HighlightStyle, Inline};

#[test]
fn task_markers_survive_block_content_without_duplicates() {
	for (source, expected) in [
		("- [x] # title\n", "☑ # title"),
		("- [ ] \n  - child\n", "☐ child"),
		("- [x] > quote\n", "☑ > quote"),
		("9. [X] # title\n", "☑ # title"),
		("- [x] **done**\n", "☑ done"),
		("- [ ]\n- [x]\n", "☐ ☑ "),
		("- \\[x] literal\n", "[x] literal"),
		("-     [x] code, not a task\n", "[x] code, not a task\n"),
	] {
		let mut rendered = Inline::default();

		markdown::append_inline(
			&markdown::parse(source),
			HighlightStyle::default(),
			None,
			&mut rendered,
		);

		assert_eq!(rendered.text, expected, "{source:?}");
	}
}
