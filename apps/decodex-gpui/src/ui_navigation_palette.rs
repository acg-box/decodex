//! Theme-owned section symbols and neutral navigation headings.
use gpui::Rgba;

use super::{ColorScheme, TEXT, color_themes::mix};

impl ColorScheme {
	pub(crate) fn navigation_heading(self) -> Rgba {
		self.rgb(TEXT)
	}

	pub(crate) fn navigation_slot(self, slot: usize) -> Rgba {
		let base = self.palette().navigation_bases[slot];
		gpui::rgb(mix(base, self.hex(TEXT), 18))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ui_theme::{ColorTheme, SIDEBAR_MATERIAL, appearance::Theme};

	#[test]
	fn navigation_labels_and_section_icons_have_contrast_in_every_palette() {
		fn luminance(color: Rgba) -> f32 {
			let linear = |v: f32| {
				if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
			};
			0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
		}
		for appearance in [Theme::Light, Theme::Dark] {
			for theme in ColorTheme::ALL {
				let colors = ColorScheme::new(appearance, theme);
				let background = luminance(colors.rgb(SIDEBAR_MATERIAL >> 8));
				let label = luminance(colors.navigation_heading());
				let contrast =
					|ink: f32| (ink.max(background) + 0.05) / (ink.min(background) + 0.05);
				assert!(contrast(label) >= 4.5, "{appearance:?} {theme:?}: heading");
				for slot in 0..colors.palette().navigation_bases.len() {
					assert!(
						contrast(luminance(colors.navigation_slot(slot))) >= 3.,
						"{appearance:?} {theme:?}: slot {slot}"
					);
				}
			}
		}
	}
}
