// UI copy: use sentence case for labels, headings, tooltips and accessibility names.
// Keep product names, abbreviations and native macOS menu names in their official case.
// Use “sign in” / “sign out”, “reset card”, “conversation” and “All work” consistently.
// Describe actions and outcomes; keep transport and storage details in diagnostics.
//! Shared visual tokens for the native Decodex operating shell.
//!
//! Page owners keep their domain-specific layout. This module owns only the
//! material, color, and motion values that must remain stable across pages.

#[path = "ui_appearance.rs"] pub(crate) mod appearance;
#[path = "ui_material.rs"] pub(crate) mod material;
#[cfg(all(target_os = "macos", not(test)))]
#[path = "native_material_panel.rs"]
pub(crate) mod native_material_panel;
#[path = "window_material.rs"] pub(crate) mod window_material;
pub(crate) mod foreground {
	//! Reading roles for the workspace. Materials and composer tokens stay separate.
	pub(crate) const PRIMARY: u32 = 0xf2f1f4;
	pub(crate) const BODY: u32 = 0xdfdee4;
	pub(crate) const SECONDARY: u32 = 0xbbb8c4;
	pub(crate) const CAPTION_SIZE: f32 = super::CAPTION_SIZE;
	pub(crate) const METRIC_SIZE: f32 = 11.;
	pub(crate) const READING_SIZE: f32 = super::BODY_SIZE;
	pub(crate) const READING_LINE_HEIGHT: f32 = 20.;
}

#[path = "ui_color_theme.rs"] mod color_themes;
#[path = "ui_palette.rs"] mod palette;

pub(crate) use self::{color_themes::ColorTheme, palette::ColorScheme};
#[cfg(test)] pub(crate) use palette::MASCOT_FAMILIES;

use std::{
	sync::{Arc, OnceLock},
	time::Duration,
};

use gpui::{
	self, AnyElement, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
	Rgba, Role, Stateful, StatefulInteractiveElement as _, Styled as _,
	prelude::FluentBuilder as _,
};

use appearance::Theme;
use material::Surface;

pub(crate) const FONT_FAMILY: &str = ".SystemUIFont";
pub(crate) const BODY_SIZE: f32 = 12.5;
pub(crate) const CAPTION_SIZE: f32 = 10.5;
pub(crate) const CAPTION_LINE_HEIGHT: f32 = 14.;
pub(crate) const HEADING_SIZE: f32 = 15.0;
pub(crate) const BODY_LINE_HEIGHT: f32 = 19.0;
pub(crate) const TREE_ROW_HEIGHT: f32 = 24.0;
pub(crate) const CONVERSATION_TAB_SIZE: f32 = 28.0;
pub(crate) const CONVERSATION_TAB_GAP: f32 = 2.0;
pub(crate) const SIDEBAR_INSET: f32 = 6.0;
pub(crate) const MESSAGE_GAP: f32 = 20.0;
pub(crate) const USER_MESSAGE_INSET: f32 = 12.0;
pub(crate) const USER_MESSAGE_ACTION_SIZE: f32 = 24.0;
pub(crate) const METADATA_GAP: f32 = 4.0;
pub(crate) const CONTROL_SIZE: f32 = 28.0;
pub(crate) const CHROME_CONTROL_SIZE: f32 = 24.0;
pub(crate) const CONTROL_GROUP_HEIGHT: f32 = 28.0;
pub(crate) const CONTROL_MARGIN: f32 = 8.0;
pub(crate) const CONTROL_RADIUS: f32 = 8.0;
pub(crate) const POPOVER_RADIUS: f32 = 10.;
pub(crate) const POPOVER_PADDING: f32 = 8.;
pub(crate) const POPOVER_CONTENT_PADDING: f32 = 6.;
pub(crate) const POPOVER_GAP: f32 = 8.;
pub(crate) const POPOVER_ARRIVAL: Duration = Duration::from_millis(160);
pub(crate) const POPOVER_OFFSET: f32 = 4.;
pub(crate) const WORKSPACE_GUTTER: f32 = 8.0;
pub(crate) const WORKSPACE_RADIUS: f32 = 16.0;
pub(crate) const CONVERSATION_HEADER_HEIGHT: f32 = 36.0;
// Conversation text and composer share a bounded, responsive reading column.
pub(crate) const CONVERSATION_WIDTH: f32 = 880.0;
pub(crate) const CONVERSATION_INSET: f32 = 16.0;
pub(crate) const COMPOSER_RADIUS: f32 = 14.0;
// A one-line editor and its toolbar keep the same footprint in read-only states.
pub(crate) const COMPOSER_MIN_HEIGHT: f32 = 77.0;
pub(crate) const COMPOSER_TOP_GAP: f32 = 8.0;
pub(crate) const COMPOSER_BOTTOM_GAP: f32 = 12.0;
// Settings share shell typography and a bounded reading width.
pub(crate) const SETTINGS_WIDTH: f32 = 740.0;
// Leave room for navigation, page insets, five theme previews and their scrollbar.
pub(crate) const SETTINGS_WINDOW_MIN_WIDTH: f32 = 920.0;
pub(crate) const SETTINGS_INSET: f32 = 24.0;
pub(crate) const SETTINGS_TOP: f32 = 16.0;
pub(crate) const SETTINGS_GROUP_GAP: f32 = 18.0;
pub(crate) const SETTINGS_LABEL_SIZE: f32 = 12.5;
pub(crate) const SETTINGS_HELP_SIZE: f32 = 11.5;
pub(crate) const SETTINGS_SECTION_SIZE: f32 = 13.0;
pub(crate) const SETTINGS_CONTROL_HEIGHT: f32 = 24.0;
pub(crate) const SETTINGS_CONTROL_RADIUS: f32 = 5.0;
pub(crate) const PANEL_DIVIDER: u32 = 0xffffff10;
// Color values below are stable role IDs. ColorScheme resolves display colors.
pub(crate) const CANVAS: u32 = 0x0b0a0f;
// One bounded glass hierarchy. Large regions always own a material, while
// nested components target a final composite opacity instead of repeating the
// same local alpha. This avoids both unreadable bare blur and opaque stacks of
// translucent black.
pub(crate) const SHELL_MATERIAL: u32 = 0x10101459;
pub(crate) const CONTENT_MATERIAL: u32 = 0x14141978;
pub(crate) const TOPBAR_MATERIAL: u32 = 0x15151b68;
// Agent sidebar is a direct child of the shell, never a child of content tint.
pub(crate) const AGENT_SIDEBAR_MATERIAL: u32 = 0x17171c58;
pub(crate) const AGENT_CHAT_OVERLAY: u32 = 0x17171c0e;
pub(crate) const SIDEBAR_MATERIAL: u32 = 0x100e1584;
pub(crate) const SURFACE_RAISED_MATERIAL: u32 = 0x17151e46;
pub(crate) const COMPOSER_MATERIAL: u32 = 0x22222888;
pub(crate) const MESSAGE_MATERIAL: u32 = 0x222c3a94;
// Neutral feedback brightens the existing material without replacing it with an opaque tile.
pub(crate) const HOVER_FILL: u32 = 0xffffff0c;
pub(crate) const PRESSED_FILL: u32 = 0xffffff18;
pub(crate) const SELECTED_FILL: u32 = 0xe49a7024;
pub(crate) const SELECTED_HOVER_FILL: u32 = 0xe49a7030;
pub(crate) const SELECTED_BORDER: u32 = 0xe49a7088;
pub(crate) const CONTROL_SELECTED_FILL: u32 = 0xf2f1f418;
pub(crate) const CONTROL_SELECTED_HOVER: u32 = 0xf2f1f430;
pub(crate) const CONTROL_SELECTED_BORDER: u32 = 0xf2f1f40c;
pub(crate) const FIELD_MATERIAL: u32 = 0xffffff08;
pub(crate) const SURFACE_OVERLAY_MATERIAL: u32 = 0x1d1a2470;
pub(crate) const LINE_STRONG: u32 = 0x403b48;
pub(crate) const PANEL_HEADER_TINT: u32 = 0xffffff05;
pub(crate) const TEXT: u32 = 0xeeeaf0;
pub(crate) const TEXT_MUTED: u32 = 0xaaa4af;
pub(crate) const TEXT_FAINT: u32 = 0xaaa4af;
pub(crate) const ACCENT: u32 = 0xe49a70;
// Primary surfaces are independent of the text and selection accent.
pub(crate) const PRIMARY_FILL: u32 = 0xd9363e;
pub(crate) const CONTROL_ACTIVE_FILL: u32 = 0x375ca8;
pub(crate) const ON_PRIMARY: u32 = 0xfdfdfd;
pub(crate) const BLUE: u32 = 0x8baaf7;
pub(crate) const GREEN: u32 = 0x77c99a;
pub(crate) const AMBER: u32 = 0xe0b56f;
// Semantic roles stay separate from chart hues and primary button colors.
pub(crate) const INFO: u32 = 0x6495ed;
pub(crate) const SUCCESS: u32 = 0x2e8b57;
pub(crate) const WARNING: u32 = 0xdaa520;
pub(crate) const ERROR: u32 = 0xef4444;
pub(crate) const MOTION_PANEL: Duration = Duration::from_millis(240);
/// Stable gutter shared by conversation history and composer alignment.
pub(crate) const HISTORY_RAIL_WIDTH: f32 = CONVERSATION_INSET;

/// The reading plane keeps its tone when the outer frame becomes translucent.
pub(crate) struct WorkspacePalette {
	pub(crate) graph: Rgba,
	pub(crate) conversation: Rgba,
}

pub(crate) fn rgb(token: u32) -> Rgba {
	ColorScheme::current().rgb(token)
}

/// Keep changing numeric values aligned without changing their text face.
pub(crate) fn tabular_figures() -> gpui::FontFeatures {
	static FEATURES: OnceLock<gpui::FontFeatures> = OnceLock::new();
	FEATURES.get_or_init(|| gpui::FontFeatures(Arc::new(vec![("tnum".into(), 1)]))).clone()
}

/// Content annotations remain readable without adding a glass layer per Agent.
pub(crate) fn annotation_surface() -> Rgba {
	ColorScheme::current().annotation()
}

/// Inline menus retain a stable reading surface when native glass is unavailable.
pub(crate) fn popover_surface() -> Rgba {
	rgb(SURFACE_OVERLAY_MATERIAL >> 8)
}

/// Thin and Regular follow the theme; Clear keeps the native glass unmodified.
pub(crate) fn popover_tint(
	material: material::Material,
	style: material::GlassStyle,
) -> Option<Rgba> {
	match (material, style) {
		(material::Material::Thin, _) => Some(popover_surface()),
		(_, material::GlassStyle::Regular) => Some(popover_surface().opacity(0.12)),
		(_, material::GlassStyle::Clear) => None,
	}
}

/// Menu rows use neutral feedback; selection is a check, danger uses ERROR text.
/// Primary actions keep their filled treatment outside selection menus.
pub(crate) fn menu_row(id: impl Into<gpui::SharedString>, enabled: bool) -> Stateful<Div> {
	gpui::div()
		.id(id.into())
		.role(Role::Button)
		.tab_index(0)
		.tab_stop(enabled)
		.w_full()
		.min_w_0()
		.min_h(gpui::px(CONTROL_SIZE))
		.px(gpui::px(8.))
		.py(gpui::px(4.))
		.rounded(gpui::px(6.))
		.text_size(gpui::px(12.))
		.line_height(gpui::px(18.))
		.text_color(rgb(TEXT))
		.when(enabled, |d| {
			d.cursor_pointer()
				.hover(|s| s.bg(rgba(HOVER_FILL)))
				.active(|s| s.bg(rgba(PRESSED_FILL)))
				.focus_visible(|s| s.bg(rgba(HOVER_FILL)))
		})
}

/// A restrained edge separates the foreground from either native material.
pub(crate) fn menu_outline<T: gpui::Styled>(surface: T) -> T {
	surface
		.rounded(gpui::px(POPOVER_RADIUS))
		.border(gpui::px(0.5))
		.border_color(rgb(TEXT).opacity(0.1))
}

pub(crate) fn menu_separator() -> Div {
	gpui::div().h(gpui::px(0.5)).mx(gpui::px(8.)).my(gpui::px(3.)).bg(rgb(TEXT).opacity(0.1))
}

pub(crate) fn menu_section(label: &'static str, content: impl IntoElement) -> Div {
	gpui::div()
		.flex()
		.flex_col()
		.gap(gpui::px(3.))
		.child(
			gpui::div()
				.px(gpui::px(8.))
				.text_size(gpui::px(CAPTION_SIZE))
				.text_color(rgb(TEXT_MUTED))
				.child(label),
		)
		.child(content)
}

pub(crate) fn rgba(token: u32) -> Rgba {
	let colors = ColorScheme::current();
	let surface = match token {
		COMPOSER_MATERIAL => Surface::InputBar,
		SHELL_MATERIAL
		| SIDEBAR_MATERIAL
		| AGENT_SIDEBAR_MATERIAL
		| TOPBAR_MATERIAL
		| CONTENT_MATERIAL
		| AGENT_CHAT_OVERLAY
		| SURFACE_OVERLAY_MATERIAL => Surface::Window,
		_ => return colors.rgba(token),
	};

	colors.material(token, surface.material())
}

pub(crate) fn workspace_palette() -> WorkspacePalette {
	ColorScheme::current().workspace(Surface::Window.material())
}

pub(crate) fn floating_group() -> Div {
	gpui::div()
		.h(gpui::px(CONTROL_GROUP_HEIGHT))
		.flex_none()
		.px_1()
		.flex()
		.items_center()
		.gap_1()
		.rounded(gpui::px(CONTROL_RADIUS))
		.bg(rgb(TOPBAR_MATERIAL >> 8))
		.border_1()
		.border_color(rgba(0xffffff12))
}

/// Graph controls share shell geometry, with a clear surface of their own.
pub(crate) fn canvas_control_group() -> Div {
	floating_group()
		.rounded(gpui::px(10.))
		.border_color(gpui::transparent_black())
		.bg(rgba(FIELD_MATERIAL))
}

/// Workspace columns share quiet framing while their content keeps its own palette.
pub(crate) fn workspace_surface() -> Div {
	gpui::div()
		.relative()
		.rounded(gpui::px(WORKSPACE_RADIUS))
		.bg(rgba(FIELD_MATERIAL))
		.overflow_hidden()
}

pub(crate) fn settings_header_inset() -> Div {
	gpui::div().px(gpui::px(SETTINGS_INSET)).pt(gpui::px(SETTINGS_TOP)).flex().justify_center()
}

pub(crate) fn settings_row() -> Div {
	gpui::div()
		.w_full()
		.min_h(gpui::px(48.0))
		.py(gpui::px(8.0))
		.flex()
		.items_center()
		.gap(gpui::px(16.0))
		.text_size(gpui::px(SETTINGS_LABEL_SIZE))
		.line_height(gpui::px(18.0))
		.text_color(rgb(TEXT))
}

pub(crate) fn settings_section_title(title: &'static str) -> Stateful<Div> {
	gpui::div()
		.id(gpui::SharedString::from(format!("settings-group-{title}")))
		.role(Role::Heading)
		.aria_level(2)
		.aria_label(title)
		.text_size(gpui::px(SETTINGS_SECTION_SIZE))
		.line_height(gpui::px(18.0))
		.font_weight(FontWeight::MEDIUM)
		.text_color(rgb(TEXT))
		.child(title)
}

pub(crate) fn settings_description(text: impl Into<gpui::SharedString>) -> Div {
	gpui::div()
		.text_size(gpui::px(SETTINGS_HELP_SIZE))
		.line_height(gpui::px(16.5))
		.text_color(rgb(TEXT_MUTED))
		.child(text.into())
}

pub(crate) fn settings_title(title: &'static str) -> impl IntoElement {
	gpui::div()
		.id(title)
		.role(Role::Heading)
		.aria_level(1)
		.aria_label(title)
		.text_size(gpui::px(18.0))
		.line_height(gpui::px(24.0))
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(rgb(TEXT))
		.child(title)
}

pub(crate) fn settings_label(
	title: impl Into<gpui::SharedString>,
	description: impl Into<gpui::SharedString>,
) -> Div {
	let description = description.into();

	gpui::div()
		.flex_1()
		.min_w_0()
		.flex()
		.flex_col()
		.gap(gpui::px(2.0))
		.child(
			gpui::div()
				.text_size(gpui::px(SETTINGS_LABEL_SIZE))
				.line_height(gpui::px(18.0))
				.child(title.into()),
		)
		.when(!description.is_empty(), |label| label.child(settings_description(description)))
}

pub(crate) fn settings_group(title: &'static str, rows: Vec<AnyElement>) -> Div {
	gpui::div()
		.w_full()
		.flex()
		.flex_col()
		.gap_2()
		.child(settings_section_title(title))
		.child(gpui::div().w_full().min_w_0().flex().flex_col().children(rows))
}

pub(crate) fn settings_button(
	id: impl Into<gpui::SharedString>,
	label: impl Into<gpui::SharedString>,
) -> Stateful<Div> {
	settings_button_base(id, label)
		.cursor_pointer()
		.hover(|s| s.bg(rgba(0xffffff0e)))
		.active(|s| s.bg(rgba(PRESSED_FILL)))
		.focus_visible(|s| s.border_color(rgb(ACCENT)))
}

pub(crate) fn settings_button_base(
	id: impl Into<gpui::SharedString>,
	label: impl Into<gpui::SharedString>,
) -> Stateful<Div> {
	let id = id.into();
	let label = label.into();

	gpui::div()
		.id(id.clone())
		.debug_selector(move || id.to_string())
		.role(Role::Button)
		.aria_label(label.clone())
		.tab_index(0)
		.h(gpui::px(SETTINGS_CONTROL_HEIGHT))
		.px(gpui::px(8.0))
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
		.rounded(gpui::px(SETTINGS_CONTROL_RADIUS))
		.border_1()
		.border_color(rgba(0xffffff16))
		.bg(rgba(0xffffff06))
		.text_size(gpui::px(11.5))
		.line_height(gpui::px(16.0))
		.text_color(rgb(TEXT))
		.child(label)
}

pub(crate) fn settings_button_group(label: &'static str) -> Stateful<Div> {
	let light = Theme::current() == Theme::Light;

	gpui::div()
		.id(gpui::SharedString::from(format!("settings-segments-{label}")))
		.role(Role::Group)
		.aria_label(label)
		.flex()
		.flex_none()
		.gap(gpui::px(2.0))
		.p(gpui::px(2.0))
		.rounded(gpui::px(6.))
		.bg(gpui::rgba(if light { 0x00000008 } else { 0x00000020 }))
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::{Context, Render, TestAppContext, Window};

	struct Menu;
	impl Render for Menu {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			gpui::div().flex().flex_col().children([
				menu_row("enabled-first", true)
					.debug_selector(|| "enabled-first".into())
					.child("First"),
				menu_row("disabled", false).child("Unavailable"),
				menu_row("enabled-last", true)
					.debug_selector(|| "enabled-last".into())
					.child("Last"),
			])
		}
	}

	#[gpui::test]
	fn menu_keyboard_navigation_skips_disabled_rows_in_both_directions(cx: &mut TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, _| Menu);
		visual.update(crate::ui_frame::draw_current_frame);
		let first = visual.debug_bounds("enabled-first").unwrap().center();
		let last = visual.debug_bounds("enabled-last").unwrap().center();
		visual.simulate_click(first, Default::default());
		let first = visual.update(|window, cx| window.focused(cx).expect("first row focus"));
		visual.simulate_click(last, Default::default());
		let last = visual.update(|window, cx| window.focused(cx).expect("last row focus"));
		visual.update(|window, cx| {
			assert!(!first.is_focused(window));
			window.focus_next(cx);
			assert!(first.is_focused(window), "forward navigation must wrap past disabled rows");
			window.focus_prev(cx);
			assert!(last.is_focused(window), "reverse navigation must skip disabled rows");
		});
	}
}
