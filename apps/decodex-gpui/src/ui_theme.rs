//! Shared visual tokens for the native Decodex operating shell.
//!
//! Page owners keep their domain-specific layout. This module owns only the
//! material, color, and motion values that must remain stable across pages.

#[cfg(all(target_os = "macos", not(test)))]
#[path = "native_glass_panel.rs"]
pub(crate) mod native_glass_panel;
#[path = "window_material.rs"] pub(crate) mod window_material;

use std::time::Duration;

use gpui::{
	self, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Role,
	StatefulInteractiveElement as _, Styled as _,
};

pub(crate) const FONT_FAMILY: &str = ".SystemUIFont";
pub(crate) const BODY_SIZE: f32 = 12.5;
pub(crate) const CAPTION_SIZE: f32 = 10.5;
pub(crate) const HEADING_SIZE: f32 = 15.0;
pub(crate) const BODY_LINE_HEIGHT: f32 = 19.0;
pub(crate) const PANEL_HEADER_HEIGHT: f32 = 30.0;
pub(crate) const TREE_ROW_HEIGHT: f32 = 24.0;
pub(crate) const MESSAGE_GAP: f32 = 20.0;
pub(crate) const USER_MESSAGE_ACTION_SIZE: f32 = 24.0;
pub(crate) const METADATA_GAP: f32 = 4.0;
pub(crate) const CONTROL_SIZE: f32 = 28.0;
pub(crate) const CHROME_CONTROL_SIZE: f32 = 24.0;
pub(crate) const CONTROL_GROUP_HEIGHT: f32 = 28.0;
pub(crate) const CONTROL_MARGIN: f32 = 8.0;
pub(crate) const CONTROL_RADIUS: f32 = 8.0;
// Settings share shell typography and a bounded reading width.
pub(crate) const SETTINGS_WIDTH: f32 = 680.0;
pub(crate) const SETTINGS_INSET: f32 = 24.0;
pub(crate) const SETTINGS_TOP: f32 = 12.0;
pub(crate) const SETTINGS_GROUP_GAP: f32 = 16.0;
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
pub(crate) const SELECTED_HOVER_FILL: u32 = 0xffffff1b;
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

pub(crate) fn floating_group() -> Div {
	gpui::div()
		.h(gpui::px(CONTROL_GROUP_HEIGHT))
		.flex_none()
		.px_1()
		.flex()
		.items_center()
		.gap_1()
		.rounded(gpui::px(CONTROL_RADIUS))
		.bg(gpui::rgba(TOPBAR_MATERIAL))
		.border_1()
		.border_color(gpui::rgba(0xffffff12))
}

pub(crate) fn settings_header_inset() -> Div {
	gpui::div().px(gpui::px(SETTINGS_INSET)).pt(gpui::px(SETTINGS_TOP)).flex().justify_center()
}

pub(crate) fn settings_row() -> Div {
	gpui::div()
		.w_full()
		.min_h(gpui::px(44.0))
		.px(gpui::px(12.0))
		.py(gpui::px(7.0))
		.flex()
		.items_center()
		.gap(gpui::px(16.0))
}

pub(crate) fn settings_title(title: &'static str) -> impl IntoElement {
	gpui::div()
		.id(title)
		.role(Role::Heading)
		.aria_level(1)
		.aria_label(title)
		.text_size(gpui::px(HEADING_SIZE))
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(gpui::rgb(TEXT))
		.child(title)
}
