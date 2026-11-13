//! One outline family for navigation, scoped creation and row actions.
use gpui::{App, IntoElement, RenderOnce, Window, prelude::Styled as _};
#[derive(Clone, Copy)]
pub(super) enum Glyph {
	Pin,
	Assistant,
	Folder,
	Sort,
	Plus,
	Search,
	Archive,
	Trash,
	Restore,
	Back,
}
#[derive(IntoElement)]
pub(super) struct Icon {
	pub glyph: Glyph,
	pub size: f32,
}
impl RenderOnce for Icon {
	fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
		if matches!(self.glyph, Glyph::Archive) {
			return crate::shell::workspace_symbols::icon_sized(
				crate::shell::workspace_symbols::Symbol::Archive,
				self.size,
			);
		}
		let path: &'static [u8] = match self.glyph {
            Glyph::Pin => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M5 2h6M5.5 2l-.5 6-2 2v1h10v-1l-2-2-.5-6M8 11v3" fill="none" stroke="white" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
            Glyph::Assistant => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M7 2l1.5 4.5L13 8l-4.5 1.5L7 14 5.5 9.5 1 8l4.5-1.5L7 2zM12 1v3M10.5 2.5h3" fill="none" stroke="white" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
            Glyph::Folder => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 4V3h4l2 2h6v8H2V4z" fill="none" stroke="white" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
            Glyph::Sort => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 5h5m3 0h4M2 11h2m3 0h7M7 3v4M4 9v4" fill="none" stroke="white" stroke-width="1.25" stroke-linecap="round"/></svg>"#,
            Glyph::Plus => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M3 8h10M8 3v10" fill="none" stroke="white" stroke-width="1.4" stroke-linecap="round"/></svg>"#,
            Glyph::Search => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="7" cy="7" r="5" fill="none" stroke="white" stroke-width="1.25"/><path d="M11 11l3 3" stroke="white" stroke-width="1.25" stroke-linecap="round"/></svg>"#,
            Glyph::Archive => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 3h12v3H2V3zm1 3v7h10V6M6 8h4" fill="none" stroke="white" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
            Glyph::Trash => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 4h12M6 4V2h4v2M4 4l1 10h6l1-10M6.5 6.5v5M9.5 6.5v5" fill="none" stroke="white" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
            Glyph::Restore => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M3 7h10v7H3V7zM8 10V2M5 5l3-3 3 3" fill="none" stroke="white" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
            Glyph::Back => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M13 8H3m4-4L3 8l4 4" fill="none" stroke="white" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
        };
		gpui::svg()
			.data(path)
			.size(gpui::px(self.size))
			.flex_none()
			.text_color(window.text_style().color)
			.into_any_element()
	}
}
