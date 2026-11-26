//! A fixed first grapheme and a clipped title share one layout during sidebar motion.
use gpui::{
	App, FontWeight, IntoElement, RenderOnce, SharedString, Window,
	prelude::{InteractiveElement as _, ParentElement as _, Styled as _},
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::ui_theme;

/// The first complete visible character, including combined accents and emoji.
fn first_grapheme(label: &str) -> &str {
	label.graphemes(true).next().unwrap_or("?")
}

/// A compact identity that fits the existing centered navigation slot.
#[derive(IntoElement)]
pub(crate) struct RailInitial {
	pub id: String,
	pub label: String,
}
impl RenderOnce for RailInitial {
	fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
		let initial = first_grapheme(self.label.trim_start()).to_owned();
		gpui::div()
			.id(SharedString::from(self.id.clone()))
			.debug_selector(move || self.id.clone())
			.flex_none()
			.whitespace_nowrap()
			.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
			.font_weight(FontWeight::MEDIUM)
			.child(initial)
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn compact_initial_preserves_complete_unicode_characters() {
		for (label, initial) in [
			("Decodex", "D"),
			("研发计划", "研"),
			("e\u{301}lan", "e\u{301}"),
			("👨‍👩‍👧‍👦 Family", "👨‍👩‍👧‍👦"),
			("", "?"),
		] {
			assert_eq!(super::first_grapheme(label), initial);
		}
	}
}
