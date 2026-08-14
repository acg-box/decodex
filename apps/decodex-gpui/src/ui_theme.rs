//! Shared visual tokens for the native Decodex operating shell.
//!
//! Page owners keep their domain-specific layout. This module owns only the
//! material, color, and motion values that must remain stable across pages.

use std::time::Duration;

pub(crate) const CANVAS: u32 = 0x0b0a0f;
// One bounded glass hierarchy. Large regions always own a material, while
// nested components target a final composite opacity instead of repeating the
// same local alpha. This avoids both unreadable bare blur and opaque stacks of
// translucent black.
pub(crate) const SHELL_MATERIAL: u32 = 0x0b0a0f4c;
pub(crate) const CONTENT_MATERIAL: u32 = 0x0d0c1278;
pub(crate) const TOPBAR_MATERIAL: u32 = 0x0c0b1058;
pub(crate) const SIDEBAR_MATERIAL: u32 = 0x100e1584;
pub(crate) const SURFACE_OVERLAY: u32 = 0x1d1a24;
pub(crate) const SURFACE_MATERIAL: u32 = 0x100e152a;
pub(crate) const SURFACE_RAISED_MATERIAL: u32 = 0x17151e46;
pub(crate) const COMPOSER_MATERIAL: u32 = 0x17151e56;
pub(crate) const FIELD_MATERIAL: u32 = 0xffffff08;
pub(crate) const SURFACE_OVERLAY_MATERIAL: u32 = 0x1d1a2470;

pub(crate) const LINE: u32 = 0x2a2730;
pub(crate) const LINE_STRONG: u32 = 0x403b48;
pub(crate) const TEXT: u32 = 0xeeeaf0;
pub(crate) const TEXT_MUTED: u32 = 0xaaa4af;
pub(crate) const TEXT_FAINT: u32 = 0x706b76;
pub(crate) const ACCENT: u32 = 0xe49a70;
pub(crate) const BLUE: u32 = 0x8baaf7;
pub(crate) const GREEN: u32 = 0x77c99a;
pub(crate) const AMBER: u32 = 0xe0b56f;

pub(crate) const MOTION_PANEL: Duration = Duration::from_millis(240);

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn nested_shell_materials_keep_a_visible_blur_budget() {
		for material in [
			SHELL_MATERIAL,
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
			assert!(alpha <= 0x8f, "nested material must not erase the blurred window");
		}

		fn composite(under: f32, over: u32) -> f32 {
			let over = (over & 0xff) as f32 / 255.0;
			over + under * (1.0 - over)
		}

		let window = (SHELL_MATERIAL & 0xff) as f32 / 255.0;
		let page = composite(window, CONTENT_MATERIAL);
		let pane = composite(window, SIDEBAR_MATERIAL);
		let surface = composite(page, SURFACE_MATERIAL);
		let raised = composite(page, SURFACE_RAISED_MATERIAL);
		let composer = composite(page, COMPOSER_MATERIAL);
		let overlay = composite(page, SURFACE_OVERLAY_MATERIAL);
		assert!((0.28..=0.32).contains(&window));
		assert!((0.60..=0.65).contains(&page));
		assert!((0.64..=0.68).contains(&pane));
		assert!((0.67..=0.71).contains(&surface));
		assert!((0.71..=0.75).contains(&raised));
		assert!((0.73..=0.77).contains(&composer));
		assert!((0.77..=0.81).contains(&overlay));
		assert!(page < surface && surface < raised && raised < composer && composer < overlay);
	}

	#[test]
	fn panel_motion_is_perceptible_without_delaying_work() {
		assert!(MOTION_PANEL >= Duration::from_millis(220));
		assert!(MOTION_PANEL <= Duration::from_millis(280));
	}
}
