//! Theme-owned section symbols and neutral navigation headings.
use gpui::Rgba;

use super::{ColorScheme, TEXT, color_themes::mix};

impl ColorScheme {
	pub(crate) fn navigation_heading(self) -> Rgba {
		self.rgb(TEXT)
	}

	pub(crate) fn navigation_slot(self, slot: usize) -> Rgba {
		let base = if self.is_light() {
			[0x4a6c92, 0x4f7c78, 0x64764b, 0x8c7243, 0x956b57, 0x946b80, 0x776294, 0x626f85][slot]
		} else {
			self.color_theme().palette().navigation_bases[slot]
		};
		gpui::rgb(mix(base, self.hex(TEXT), 18))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ui_theme::{ColorTheme, SIDEBAR_MATERIAL, appearance::Theme};

	#[test]
	fn navigation_headings_remain_readable_in_every_theme() {
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
				let contrast = (label.max(background) + 0.05) / (label.min(background) + 0.05);
				assert!(contrast >= 4.5, "{appearance:?} {theme:?}: {contrast}");
			}
		}
	}
}
