//! Theme-owned navigation colors. Identity slots do not depend on list order or titles.
use gpui::Rgba;

use super::{ColorScheme, TEXT, TEXT_MUTED, color_themes::mix};

#[derive(Clone, Copy)]
pub(crate) struct NavigationIdentity {
	pub(crate) ink: Rgba,
	pub(crate) label: Rgba,
}

impl ColorScheme {
	pub(crate) fn navigation_heading(self) -> Rgba {
		gpui::rgb(mix(self.hex(TEXT_MUTED), self.hex(TEXT), 22))
	}

	pub(crate) fn navigation_project(self, id: &str) -> NavigationIdentity {
		if id.is_empty() {
			return NavigationIdentity {
				ink: self.rgb(TEXT_MUTED),
				label: self.navigation_heading(),
			};
		}
		self.navigation_slot(identity_slot(id))
	}

	pub(crate) fn navigation_slot(self, slot: usize) -> NavigationIdentity {
		let base = if self.is_light() {
			[0x4a6c92, 0x4f7c78, 0x64764b, 0x8c7243, 0x956b57, 0x946b80, 0x776294, 0x626f85][slot]
		} else {
			self.color_theme().palette().navigation_bases[slot]
		};
		let text = self.hex(TEXT);

		NavigationIdentity {
			ink: gpui::rgb(mix(base, text, 18)),
			label: gpui::rgb(mix(base, text, 50)),
		}
	}
}

fn identity_slot(id: &str) -> usize {
	let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
		(hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
	});
	(hash % 8) as usize
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ui_theme::{ColorTheme, SIDEBAR_MATERIAL, appearance::Theme};

	#[test]
	fn identity_slots_are_stable_and_cover_the_palette() {
		let ids: Vec<_> = (0..64).map(|index| format!("project-{index}")).collect();
		let before: Vec<_> = ids.iter().map(|id| identity_slot(id)).collect();
		let mut reversed = ids.clone();
		reversed.reverse();
		for id in reversed {
			let original = ids.iter().position(|value| value == &id).unwrap();
			assert_eq!(identity_slot(&id), before[original]);
		}
		assert_eq!(before.iter().collect::<std::collections::HashSet<_>>().len(), 8);
	}

	#[test]
	fn project_labels_remain_readable_in_every_theme() {
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
				for slot in 0..8 {
					let label = luminance(colors.navigation_slot(slot).label);
					let contrast = (label.max(background) + 0.05) / (label.min(background) + 0.05);
					assert!(contrast >= 4.5, "{appearance:?} {theme:?} slot {slot}: {contrast}");
				}
			}
		}
	}
}
