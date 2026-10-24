// UI copy: use sentence case for labels, headings, tooltips and accessibility names.
// Keep product names, abbreviations and native macOS menu names in their official case.
// Use “sign in” / “sign out”, “reset card”, “conversation” and “All work” consistently.
// Describe actions and outcomes; keep transport and storage details in diagnostics.
//! Shared visual tokens for the native Decodex operating shell.
//!
//! Page owners keep their domain-specific layout. This module owns only the
//! material, color, and motion values that must remain stable across pages.

#[path = "ui_material.rs"] pub(crate) mod material;
#[cfg(all(target_os = "macos", not(test)))]
#[path = "native_material_panel.rs"]
pub(crate) mod native_material_panel;
#[path = "window_material.rs"] pub(crate) mod window_material;

#[path = "ui_appearance.rs"] pub(crate) mod appearance;
#[path = "ui_palette.rs"] mod palette;
pub(crate) use palette::ColorScheme;
#[path = "ui_color_theme.rs"] mod color_themes;
pub(crate) use color_themes::ColorTheme;

use std::time::Duration;

use gpui::{
	self, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Role,
	StatefulInteractiveElement as _, Styled as _,
};

pub(crate) fn rgb(token: u32) -> gpui::Rgba {
	ColorScheme::current().rgb(token)
}

pub(crate) fn rgba(token: u32) -> gpui::Rgba {
	ColorScheme::current().rgba(token)
}

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
pub(crate) const WORKSPACE_INSET: f32 = 6.0;
pub(crate) const WORKSPACE_RADIUS: f32 = 12.0;
pub(crate) const MESSAGE_GAP: f32 = 20.0;
pub(crate) const USER_MESSAGE_ACTION_SIZE: f32 = 24.0;
pub(crate) const METADATA_GAP: f32 = 4.0;
pub(crate) const CONTROL_SIZE: f32 = 28.0;
pub(crate) const CHROME_CONTROL_SIZE: f32 = 24.0;
pub(crate) const CONTROL_GROUP_HEIGHT: f32 = 28.0;
pub(crate) const CONTROL_MARGIN: f32 = 8.0;
pub(crate) const CONTROL_RADIUS: f32 = 8.0;
// Conversation text and composer share a bounded, responsive reading column.
pub(crate) const CONVERSATION_WIDTH: f32 = 880.0;
pub(crate) const CONVERSATION_INSET: f32 = 16.0;
pub(crate) const COMPOSER_RADIUS: f32 = 14.0;
pub(crate) const COMPOSER_TOP_GAP: f32 = 8.0;
pub(crate) const COMPOSER_BOTTOM_GAP: f32 = 12.0;
// Settings share shell typography and a bounded reading width.
pub(crate) const SETTINGS_WIDTH: f32 = 740.0;
pub(crate) const SETTINGS_INSET: f32 = 24.0;
pub(crate) const SETTINGS_TOP: f32 = 16.0;
pub(crate) const SETTINGS_GROUP_GAP: f32 = 18.0;
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
// Neutral feedback brightens the existing material without replacing it with an opaque tile.
pub(crate) const HOVER_FILL: u32 = 0xffffff0c;
pub(crate) const PRESSED_FILL: u32 = 0xffffff18;
pub(crate) const SELECTED_FILL: u32 = 0xe49a7018;
pub(crate) const SELECTED_HOVER_FILL: u32 = 0xe49a7024;
pub(crate) const SELECTED_BORDER: u32 = 0xe49a7066;
pub(crate) const FIELD_MATERIAL: u32 = 0xffffff08;
pub(crate) const SURFACE_OVERLAY_MATERIAL: u32 = 0x1d1a2470;
pub(crate) const LINE_STRONG: u32 = 0x403b48;
pub(crate) const PANEL_HEADER_TINT: u32 = 0xffffff05;
pub(crate) const TEXT: u32 = 0xeeeaf0;
pub(crate) const TEXT_MUTED: u32 = 0xaaa4af;
pub(crate) const TEXT_FAINT: u32 = 0xaaa4af;
pub(crate) const ACCENT: u32 = 0xe49a70;
pub(crate) const BLUE: u32 = 0x8baaf7;
pub(crate) const GREEN: u32 = 0x77c99a;
pub(crate) const AMBER: u32 = 0xe0b56f;
pub(crate) const ERROR: u32 = 0xef4444;
pub(crate) const MOTION_PANEL: Duration = Duration::from_millis(240);

/// Thin separates regions by tone; glass keeps its existing shared plane.
pub(crate) struct WorkspacePalette {
	pub(crate) graph: gpui::Rgba,
	pub(crate) conversation: gpui::Rgba,
}

pub(crate) fn workspace_palette() -> WorkspacePalette {
	if material::Surface::Window.material() == material::Material::LiquidGlass {
		return WorkspacePalette { graph: gpui::rgba(0), conversation: rgba(AGENT_CHAT_OVERLAY) };
	}
	// Neutral tints leave the native matte material underneath. Only the reading
	// plane is lighter; sidebars stay on the shared window surface.
	let (graph, conversation) = match appearance::Theme::current() {
		appearance::Theme::Dark => (0x00000028, 0xffffff06),
		appearance::Theme::Light => (0x00000008, 0xffffffb0),
	};
	WorkspacePalette { graph: gpui::rgba(graph), conversation: gpui::rgba(conversation) }
}

/// A shallow fold between the map and reading plane, without a divider stroke.
pub(crate) fn workspace_seam() -> Div {
	let alpha = match appearance::Theme::current() {
		appearance::Theme::Dark => 0x14,
		appearance::Theme::Light => 0x09,
	};
	gpui::div().absolute().top_0().left_0().right_0().h(gpui::px(6.)).bg(gpui::linear_gradient(
		180.,
		gpui::linear_color_stop(gpui::rgba(alpha), 0.),
		gpui::linear_color_stop(gpui::rgba(0), 1.),
	))
}

/// Reading roles for the workspace. Materials and composer tokens stay separate.
pub(crate) mod foreground {
	pub(crate) const PRIMARY: u32 = 0xf2f1f4;
	pub(crate) const BODY: u32 = 0xdfdee4;
	pub(crate) const SECONDARY: u32 = 0xbbb8c4;
	pub(crate) const CAPTION_SIZE: f32 = super::CAPTION_SIZE;
	pub(crate) const METRIC_SIZE: f32 = 11.;
	pub(crate) const READING_SIZE: f32 = super::BODY_SIZE;
	pub(crate) const READING_LINE_HEIGHT: f32 = 20.;
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
	floating_group().bg(gpui::transparent_black())
}

pub(crate) fn settings_header_inset() -> Div {
	gpui::div().px(gpui::px(SETTINGS_INSET)).pt(gpui::px(SETTINGS_TOP)).flex().justify_center()
}

pub(crate) fn settings_row() -> Div {
	gpui::div()
		.w_full()
		.min_h(gpui::px(50.0))
		.px(gpui::px(14.0))
		.py(gpui::px(8.0))
		.flex()
		.items_center()
		.gap(gpui::px(16.0))
		.text_size(gpui::px(BODY_SIZE))
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

pub(crate) fn settings_heading(title: &'static str, description: &'static str) -> Div {
	gpui::div().flex().flex_col().gap(gpui::px(6.0)).child(settings_title(title)).child(
		gpui::div()
			.text_size(gpui::px(11.5))
			.line_height(gpui::px(17.0))
			.text_color(rgb(TEXT_MUTED))
			.child(description),
	)
}

pub(crate) fn settings_label(
	title: &'static str,
	description: impl Into<gpui::SharedString>,
) -> Div {
	let description = description.into();
	use gpui::prelude::FluentBuilder as _;
	gpui::div()
		.flex_1()
		.min_w_0()
		.flex()
		.flex_col()
		.gap(gpui::px(2.0))
		.child(gpui::div().text_size(gpui::px(BODY_SIZE)).line_height(gpui::px(17.0)).child(title))
		.when(!description.is_empty(), |label| {
			label.child(
				gpui::div()
					.text_size(gpui::px(11.5))
					.line_height(gpui::px(16.0))
					.text_color(rgb(TEXT_MUTED))
					.child(description),
			)
		})
}

pub(crate) fn settings_card() -> Div {
	gpui::div()
		.w_full()
		.min_w_0()
		.flex()
		.flex_col()
		.rounded(gpui::px(10.0))
		.border_1()
		.border_color(rgba(0xffffff0c))
		.bg(rgba(0xffffff03))
}

pub(crate) fn settings_group(title: &'static str, rows: Vec<gpui::AnyElement>) -> Div {
	use gpui::prelude::FluentBuilder as _;
	gpui::div()
		.w_full()
		.flex()
		.flex_col()
		.gap_2()
		.child(
			gpui::div()
				.id(gpui::SharedString::from(format!("settings-group-{title}")))
				.role(Role::Heading)
				.aria_level(2)
				.text_size(gpui::px(11.5))
				.font_weight(FontWeight::MEDIUM)
				.text_color(rgb(TEXT_MUTED))
				.child(title),
		)
		.child(settings_card().children(rows.into_iter().enumerate().map(|(index, row)| {
			gpui::div()
				.when(index > 0, |row| row.border_t_1().border_color(rgba(0xffffff08)))
				.child(row)
		})))
}

pub(crate) fn settings_button(
	id: impl Into<gpui::SharedString>,
	label: &'static str,
) -> gpui::Stateful<Div> {
	settings_button_base(id, label)
		.cursor_pointer()
		.hover(|s| s.bg(rgba(0xffffff0e)))
		.active(|s| s.bg(rgba(PRESSED_FILL)))
		.focus_visible(|s| s.border_color(rgb(BLUE)))
}

pub(crate) fn settings_button_base(
	id: impl Into<gpui::SharedString>,
	label: &'static str,
) -> gpui::Stateful<Div> {
	let id = id.into();
	gpui::div()
		.id(id.clone())
		.debug_selector(move || id.to_string())
		.role(Role::Button)
		.aria_label(label)
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

pub(crate) fn settings_segments(label: &'static str) -> gpui::Stateful<Div> {
	let light = appearance::Theme::current() == appearance::Theme::Light;
	gpui::div()
		.id(gpui::SharedString::from(format!("settings-segments-{label}")))
		.role(Role::RadioGroup)
		.aria_label(label)
		.flex()
		.flex_none()
		.gap(gpui::px(2.0))
		.p(gpui::px(2.0))
		.rounded(gpui::px(6.))
		.bg(gpui::rgba(if light { 0x00000008 } else { 0x00000020 }))
}

pub(crate) fn settings_segment(
	id: impl Into<gpui::ElementId>,
	label: &'static str,
	selected: bool,
) -> gpui::Stateful<Div> {
	use gpui::prelude::FluentBuilder as _;
	let light = appearance::Theme::current() == appearance::Theme::Light;
	let selected_fill = if light { gpui::rgba(0xfffffff0) } else { rgba(0xffffff12) };
	let selected_hover = if light { gpui::rgba(0xffffffff) } else { rgba(SELECTED_HOVER_FILL) };
	gpui::div()
		.id(id)
		.role(Role::RadioButton)
		.aria_label(label)
		.aria_toggled(if selected {
			gpui::accesskit::Toggled::True
		} else {
			gpui::accesskit::Toggled::False
		})
		.tab_index(0)
		.h(gpui::px(22.))
		.px(gpui::px(8.0))
		.flex()
		.items_center()
		.justify_center()
		.rounded(gpui::px(4.))
		.border_1()
		.border_color(gpui::transparent_black())
		.text_size(gpui::px(11.5))
		.line_height(gpui::px(16.0))
		.cursor_pointer()
		.text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
		.when(selected, |s| s.bg(selected_fill).border_color(rgba(0xffffff0c)))
		.hover(move |s| s.bg(if selected { selected_hover } else { rgba(HOVER_FILL) }))
		.focus_visible(|s| s.border_color(rgb(BLUE)))
		.child(label)
}

/// Stable gutter shared by conversation history and composer alignment.
pub(crate) const HISTORY_RAIL_WIDTH: f32 = CONVERSATION_INSET;
