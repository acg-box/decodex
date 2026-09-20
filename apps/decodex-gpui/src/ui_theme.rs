//! Shared visual tokens for the native Decodex operating shell.
//!
//! Page owners keep their domain-specific layout. This module owns only the
//! material, color, and motion values that must remain stable across pages.

use std::time::Duration;

pub(crate) const FONT_FAMILY: &str = ".SystemUIFont";
pub(crate) const BODY_SIZE: f32 = 12.5;
pub(crate) const CAPTION_SIZE: f32 = 10.5;
pub(crate) const HEADING_SIZE: f32 = 15.0;

pub(crate) const BODY_LINE_HEIGHT: f32 = 19.0;
pub(crate) const PANEL_HEADER_HEIGHT: f32 = 30.0;
pub(crate) const TREE_ROW_HEIGHT: f32 = 28.0;
pub(crate) const MESSAGE_GAP: f32 = 20.0;
pub(crate) const METADATA_GAP: f32 = 6.0;

pub(crate) const CONTROL_SIZE: f32 = 28.0;
pub(crate) const CHROME_CONTROL_SIZE: f32 = 24.0;
pub(crate) const CONTROL_GROUP_HEIGHT: f32 = 28.0;
pub(crate) const CONTROL_MARGIN: f32 = 8.0;
pub(crate) const CONTROL_RADIUS: f32 = 8.0;

pub(crate) fn floating_group() -> gpui::Div {
	use gpui::{Styled, div, px, rgba};
	div()
		.h(px(CONTROL_GROUP_HEIGHT))
		.flex_none()
		.px_1()
		.flex()
		.items_center()
		.gap_1()
		.rounded(px(CONTROL_RADIUS))
		.bg(rgba(TOPBAR_MATERIAL))
		.border_1()
		.border_color(rgba(0xffffff12))
}

// Settings share shell typography and a bounded reading width.
pub(crate) const SETTINGS_WIDTH: f32 = 680.0;
pub(crate) fn settings_row() -> gpui::Div {
	use gpui::{Styled, div, px};
	div().w_full().min_h(px(44.0)).px(px(12.0)).py(px(7.0)).flex().items_center().gap(px(16.0))
}
pub(crate) fn settings_title(title: &'static str) -> impl gpui::IntoElement {
	use gpui::{
		FontWeight, Role, div,
		prelude::{InteractiveElement, ParentElement, StatefulInteractiveElement, Styled},
		px, rgb,
	};
	div()
		.id(title)
		.role(Role::Heading)
		.aria_level(1)
		.aria_label(title)
		.text_size(px(HEADING_SIZE))
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(rgb(TEXT))
		.child(title)
}

pub(crate) const CANVAS: u32 = 0x0b0a0f;
// One bounded glass hierarchy. Large regions always own a material, while
// nested components target a final composite opacity instead of repeating the
// same local alpha. This avoids both unreadable bare blur and opaque stacks of
// translucent black.
pub(crate) const SHELL_MATERIAL: u32 = 0x10101459;
pub(crate) const CONTENT_MATERIAL: u32 = 0x14141978;
pub(crate) const TOPBAR_MATERIAL: u32 = 0x15151b68;
// Chief sidebar is a direct child of the shell, never a child of content tint.
pub(crate) const CHIEF_SIDEBAR_MATERIAL: u32 = 0x17171c58;
pub(crate) const CHIEF_CHAT_OVERLAY: u32 = 0x17171c18;
pub(crate) const SIDEBAR_MATERIAL: u32 = 0x100e1584;
pub(crate) const SURFACE_MATERIAL: u32 = 0x100e152a;
pub(crate) const SURFACE_RAISED_MATERIAL: u32 = 0x17151e46;
pub(crate) const COMPOSER_MATERIAL: u32 = 0x22222888;
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

pub(crate) const MOTION_PANEL: Duration = Duration::from_millis(240);

/// Apply the same native, behind-window blur to each GPUI window independently.
#[cfg(target_os = "macos")]
pub(crate) fn configure_window_material(window: &gpui::Window) {
	use objc2_app_kit::{
		NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
		NSVisualEffectView,
	};
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
	// The live GPUI window owns this AppKit view; this callback runs on the main thread.
	let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
	let Some(native) = view.window() else { return };
	let Some(content) = native.contentView() else { return };
	for child in content.subviews().iter() {
		if let Some(effect) = child.downcast_ref::<NSVisualEffectView>() {
			effect.setMaterial(NSVisualEffectMaterial::Sidebar);
			effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
			effect.setState(NSVisualEffectState::Active);
		}
	}
}

/// Settings-only trial using public AppKit APIs. Unsupported systems retain vibrancy.
/// Reference: longbridge/gpui-component#2440 (3430e71048e275e0a8fa08f3d9c1386211887aab).
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) fn configure_settings_material(window: &mut gpui::Window) {
	use objc2::{msg_send, rc::Retained, runtime::AnyClass};
	use objc2_app_kit::NSView;
	use objc2_foundation::NSRect;
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	if std::env::var_os("DECODEX_DISABLE_LIQUID_GLASS").is_some() {
		configure_window_material(window);
		return;
	}
	let Some(class) = AnyClass::get(c"NSGlassEffectView") else {
		configure_window_material(window);
		return;
	};
	let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
	// GPUI owns the native view. AppKit retains the inserted glass for the window lifetime.
	let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
	let Some(native) = view.window() else { return };
	let Some(content) = native.contentView() else { return };
	let exists =
		content.subviews().iter().any(|child| unsafe { msg_send![&*child, isKindOfClass: class] });
	if exists {
		return;
	}
	unsafe {
		let glass: Retained<NSView> = msg_send![class, new];
		let bounds: NSRect = content.bounds();
		let _: () = msg_send![&*glass, setFrame: bounds];
		let _: () = msg_send![&*glass, setAutoresizingMask: 18usize];
		// Keep GPUI controls above glass so AppKit cannot intercept their input.
		window.set_background_appearance(gpui::WindowBackgroundAppearance::Transparent);
		let _: () = msg_send![&*content, addSubview: &*glass, positioned: -1isize, relativeTo: std::ptr::null::<NSView>()];
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn nested_shell_materials_keep_a_visible_blur_budget() {
		for material in [
			SHELL_MATERIAL,
			CHIEF_SIDEBAR_MATERIAL,
			CHIEF_CHAT_OVERLAY,
			CONTENT_MATERIAL,
			TOPBAR_MATERIAL,
			SIDEBAR_MATERIAL,
			SURFACE_MATERIAL,
			SURFACE_RAISED_MATERIAL,
			COMPOSER_MATERIAL,
			FIELD_MATERIAL,
			SURFACE_OVERLAY_MATERIAL,
		] {
			let alpha = material & 0xff;
			assert!(alpha > 0, "material must tint the blurred window");
			assert!(alpha < 0xff, "materials retain a bounded amount of background light");
		}

		fn composite(under: f32, over: u32) -> f32 {
			let over = (over & 0xff) as f32 / 255.0;
			over + under * (1.0 - over)
		}

		let window = (SHELL_MATERIAL & 0xff) as f32 / 255.0;
		let page = composite(window, CONTENT_MATERIAL);
		let pane = composite(window, SIDEBAR_MATERIAL);
		let chief_sidebar = composite(window, CHIEF_SIDEBAR_MATERIAL);
		assert!((0.56..=0.60).contains(&chief_sidebar));
		assert!(chief_sidebar < page);
		assert!((0.64..=0.68).contains(&page), "conversation must retain visible glass");
		assert!((0.66..=0.70).contains(&pane));
		let composer = composite(page, COMPOSER_MATERIAL);
		assert!(composer > page && composer < 1.0);
	}

	#[test]
	fn panel_motion_is_perceptible_without_delaying_work() {
		assert!(MOTION_PANEL >= Duration::from_millis(220));
		assert!(MOTION_PANEL <= Duration::from_millis(280));
	}
}
