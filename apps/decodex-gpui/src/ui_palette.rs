//! Paired app colors. The existing RGB values are stable color tokens, not
//! content pixels. Resolve them here so charts, text, controls and materials
//! share a palette. Keep media and authored diagram colors outside this path.
use super::{ColorTheme, appearance::Theme};

/// Capture both choices before background work or a theme-keyed cache lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ColorScheme {
	appearance: Theme,
	colors: ColorTheme,
}

impl ColorScheme {
	pub(crate) const fn new(appearance: Theme, colors: ColorTheme) -> Self {
		Self { appearance, colors }
	}

	pub(crate) fn current() -> Self {
		let appearance = Theme::current();
		let colors =
			if appearance == Theme::Light { ColorTheme::Decodex } else { ColorTheme::configured() };
		Self::new(appearance, colors)
	}

	pub(crate) fn rgb(self, token: u32) -> gpui::Rgba {
		gpui::rgb(match self.appearance {
			Theme::Dark => self.colors.palette().resolve(token),
			Theme::Light => light(token),
		})
	}

	pub(crate) fn hex(self, token: u32) -> u32 {
		u32::from(self.rgb(token)) >> 8
	}

	pub(crate) fn rgba(self, token: u32) -> gpui::Rgba {
		if self.appearance == Theme::Dark
			&& let Some(material) = self.colors.palette().material(token)
		{
			return gpui::rgba(material);
		}
		let mut color = self.rgb(token >> 8);
		color.a = (token & 0xff) as f32 / 255.;
		color
	}
}

impl From<Theme> for ColorScheme {
	fn from(appearance: Theme) -> Self {
		Self::new(appearance, ColorTheme::Decodex)
	}
}

fn light(token: u32) -> u32 {
	match token {
		// Materials retain their original alpha and glass ownership.
		0x0b0a0f => 0xf5f7fa,
		0x101014 | 0x100e15 => 0xf0f3f7,
		0x141419 | 0x141418 | 0x111113 => 0xf8f9fb,
		0x15151b | 0x17171c | 0x17151e | 0x17171a => 0xf2f5f8,
		0x222228 | 0x1d1a24 | 0x24242a | 0x242427 => 0xf7f9fb,
		0x29292d | 0x29292e | 0x27272b | 0x252830 => 0xe9edf3,
		0x302d39 | 0x30303a | 0x33333a | 0x35353c | 0x3a3a40 => 0xe1e7f0,
		0x303030 | 0x373737 | 0x464646 => 0xe2e7ee,
		0x141822 => 0xe7edf8,
		// Foreground hierarchy and neutral feedback.
		0xffffff | 0xf2f1f4 | 0xf4f2f7 | 0xeeeaf0 | 0xe7e7ea => 0x242b36,
		0xdfdee4 | 0xc5c6ce | 0xc4c4ce | 0xc0c0c5 => 0x3f4a59,
		0xbbb8c4 | 0xaaa4af | 0xc2bbc9 | 0xd5d5da => 0x5f6b7a,
		0xb5b2bd | 0x89909c | 0x817789 => 0x6b7789,
		0x78818e => 0x686d77,
		0x403b48 | 0x3c3744 | 0x515155 | 0x606064 => 0xb7c1ce,
		// Accent and state colors need contrast on the light material too.
		0x8baaf7 | 0x8aaaff | 0x7aa2ff | 0x6b9fff | 0x60a5fa => 0x365fa4,
		0x22c55e | 0x77c99a => 0x28734f,
		0xe0b56f | 0xf59e0b | 0xf5c985 | 0xd4b27c => 0x93601c,
		0xf97316 | 0xe49a70 | 0xffa45d | 0xff8a3d => 0x365fa4,
		0xed8585 | 0xdc2626 | 0xef4444 => 0xbe3e4a,
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
	use super::{
		super::{AGENT_CHAT_OVERLAY, BLUE, CANVAS, SHELL_MATERIAL, TEXT, TEXT_MUTED, rgb, rgba},
		*,
	};

	#[test]
	fn presets_resolve_shared_roles_and_leave_light_appearance_readable() {
		for colors in ColorTheme::ALL {
			let palette = colors.palette();
			let dark = ColorScheme::new(Theme::Dark, colors);
			for (token, expected) in [
				(CANVAS, palette.canvas),
				(TEXT, palette.text),
				(TEXT_MUTED, palette.text_muted),
				(BLUE, palette.blue),
			] {
				assert_eq!(dark.hex(token), expected, "{colors:?}");
			}
			assert_eq!(dark.rgba(SHELL_MATERIAL), gpui::rgba(palette.shell_material));
			assert_eq!(dark.rgba(AGENT_CHAT_OVERLAY), gpui::rgba(palette.agent_chat_overlay));
			assert_eq!(dark.rgb(0x123456), gpui::rgb(0x123456));
			assert_eq!(dark.rgba(0x12345678), gpui::rgba(0x12345678));

			let light = ColorScheme::new(Theme::Light, colors);
			assert_eq!(light.rgb(TEXT), gpui::rgb(0x242b36));
			assert_eq!(light.rgb(CANVAS), gpui::rgb(0xf5f7fa));
			assert_eq!(light.rgba(SHELL_MATERIAL).a, gpui::rgba(SHELL_MATERIAL).a);
		}
	}

	#[gpui::test]
	fn a_color_snapshot_keeps_its_palette_when_the_preference_changes(
		cx: &mut gpui::TestAppContext,
	) {
		let original = ColorTheme::configured();
		super::super::appearance::with_theme(Theme::Dark, || {
			cx.update(|cx| ColorTheme::TokyoNight.apply(cx));
			let captured = ColorScheme::current();
			cx.update(|cx| ColorTheme::Nord.apply(cx));
			assert_ne!(captured, ColorScheme::current());
			assert_eq!(captured.hex(TEXT), ColorTheme::TokyoNight.palette().text);
		});
		cx.update(|cx| original.apply(cx));
	}

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
			let colors = ColorScheme::from(theme);
			assert_ne!(
				colors.rgb(super::super::foreground::PRIMARY),
				colors.rgb(super::super::foreground::BODY)
			);
			assert_ne!(
				colors.rgb(super::super::foreground::BODY),
				colors.rgb(super::super::foreground::SECONDARY)
			);
			for token in [
				super::super::TEXT,
				super::super::foreground::PRIMARY,
				super::super::foreground::BODY,
				super::super::foreground::SECONDARY,
				super::super::TEXT_MUTED,
				super::super::BLUE,
				super::super::GREEN,
				super::super::AMBER,
				super::super::ERROR,
			] {
				for background in [super::super::CANVAS, super::super::COMPOSER_MATERIAL >> 8] {
					assert!(
						contrast(colors.rgb(token), colors.rgb(background)) >= 4.5,
						"{theme:?} {token:06x} on {background:06x}"
					);
				}
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
			assert_eq!(rgb(super::super::TEXT), gpui::rgb(0x242b36));
		});
	}
}
