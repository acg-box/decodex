//! Paired app colors. The existing RGB values are stable color tokens, not
//! content pixels. Resolve them here so charts, text, controls and materials
//! share a palette. Keep media and authored diagram colors outside this path.
use super::appearance::Theme;

pub(crate) fn rgb(token: u32) -> gpui::Rgba {
	Theme::current().rgb(token)
}

pub(crate) fn rgba(token: u32) -> gpui::Rgba {
	let mut color = rgb(token >> 8);
	color.a = (token & 0xff) as f32 / 255.;
	color
}

impl Theme {
	pub(crate) fn rgb(self, token: u32) -> gpui::Rgba {
		gpui::rgb(match self {
			Self::Dark => token,
			Self::Light => light(token),
		})
	}

	pub(crate) fn hex(self, token: u32) -> u32 {
		u32::from(self.rgb(token)) >> 8
	}
}

fn light(token: u32) -> u32 {
	match token {
		// Materials retain their original alpha and glass ownership.
		0x0b0a0f => 0xf6f5f8,
		0x101014 | 0x100e15 => 0xf4f3f6,
		0x141419 | 0x141418 | 0x111113 => 0xf8f7fa,
		0x15151b | 0x17171c | 0x17151e | 0x17171a => 0xf4f3f7,
		0x222228 | 0x1d1a24 | 0x24242a | 0x242427 => 0xf7f6f9,
		0x29292d | 0x29292e | 0x27272b | 0x252830 => 0xeae9ef,
		0x302d39 | 0x30303a | 0x33333a | 0x35353c | 0x3a3a40 => 0xe6e4ec,
		0x303030 | 0x373737 | 0x464646 => 0xe5e4e9,
		0x141822 => 0xe7ecf7,
		// Foreground hierarchy and neutral feedback.
		0xffffff | 0xf2f1f4 | 0xf4f2f7 | 0xeeeaf0 | 0xe7e7ea => 0x242329,
		0xdfdee4 | 0xc5c6ce | 0xc4c4ce | 0xc0c0c5 => 0x3f3d47,
		0xbbb8c4 | 0xaaa4af | 0xc2bbc9 | 0xd5d5da => 0x635f6e,
		0xb5b2bd | 0x89909c | 0x817789 => 0x716c7d,
		0x78818e => 0x686d77,
		0x403b48 | 0x3c3744 | 0x515155 | 0x606064 => 0xb5b1bf,
		// Accent and state colors need contrast on the light material too.
		0x8baaf7 | 0x8aaaff | 0x7aa2ff | 0x6b9fff | 0x60a5fa => 0x3b60bb,
		0x22c55e | 0x77c99a => 0x287847,
		0xe0b56f | 0xf59e0b | 0xf5c985 | 0xd4b27c => 0x956018,
		0xf97316 | 0xe49a70 | 0xffa45d | 0xff8a3d => 0xae552e,
		0xed8585 | 0xdc2626 | 0xef4444 => 0xc83232,
		0xa5a0ed | 0xaaa0c4 | 0xc4a7e7 | 0xb0a0d8 | 0xad9ae8 | 0xcac4f4 => 0x79529f,
		0xd1c2f4 => 0x5d3583,
		0x94a3b8 | 0x64748b => 0x657285,
		0x9cbacb | 0x7dc4cf | 0xd5e3f1 | 0x9ec8d9 | 0xe8f3f8 | 0xf1fcff => 0x3b7187,
		0x8b9fff | 0x788dff | 0xb0c6f3 | 0x9aafff | 0xdbe5ff | 0xc6cce0 => 0x5267b5,
		0xd0c8d9 | 0xaaa6b8 => 0x77717f,
		// Shadows, transparency and explicit content colors keep their values.
		_ => token,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn contrast(a: gpui::Rgba, b: gpui::Rgba) -> f32 {
		let luminance = |c: gpui::Rgba| {
			let linear =
				|v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
			0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
		};
		let (a, b) = (luminance(a), luminance(b));
		(a.max(b) + 0.05) / (a.min(b) + 0.05)
	}

	#[test]
	fn reading_roles_have_contrast_in_both_themes() {
		for theme in [Theme::Light, Theme::Dark] {
			for token in [
				super::super::TEXT,
				super::super::foreground::PRIMARY,
				super::super::foreground::BODY,
				super::super::foreground::SECONDARY,
				super::super::TEXT_MUTED,
				super::super::BLUE,
			] {
				assert!(
					contrast(theme.rgb(token), theme.rgb(super::super::CANVAS)) >= 4.5,
					"{theme:?} {token:06x}"
				);
			}
		}
	}

	#[test]
	fn theme_resolution_preserves_material_alpha_and_content_pixels() {
		super::super::appearance::with_theme(Theme::Light, || {
			assert_eq!(
				rgba(super::super::SHELL_MATERIAL).a,
				gpui::rgba(super::super::SHELL_MATERIAL).a
			);
			assert_eq!(rgba(0x00000024), gpui::rgba(0x00000024));
			assert_eq!(rgb(0x123456), gpui::rgb(0x123456));
			assert_eq!(rgb(super::super::TEXT), gpui::rgb(0x242329));
		});
	}
}
