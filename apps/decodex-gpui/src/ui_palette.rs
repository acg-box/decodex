//! Paired app colors. The existing RGB values are stable color tokens, not
//! content pixels. Resolve them here so charts, text, controls and materials
//! share a palette. Keep media and authored diagram colors outside this path.
use gpui::Rgba;

use crate::ui_theme::{
	AGENT_CHAT_OVERLAY, AGENT_SIDEBAR_MATERIAL, CANVAS, COMPOSER_MATERIAL, CONTENT_MATERIAL,
	CONTROL_ACTIVE_FILL, ColorTheme, ERROR, INFO, ON_PRIMARY, PRIMARY_FILL, SHELL_MATERIAL,
	SIDEBAR_MATERIAL, SUCCESS, SURFACE_OVERLAY_MATERIAL, TEXT, TOPBAR_MATERIAL, WARNING,
	WorkspacePalette, appearance::Theme, color_themes, material::Material,
};

pub(crate) const MASCOT_FAMILIES: [&str; 12] = [
	"mint",
	"lilac",
	"sky",
	"peach",
	"butter",
	"cream",
	"rose",
	"periwinkle",
	"ice",
	"sage",
	"coral",
	"midnight",
];

/// Body, face and accessory ink belong to the same character palette.
pub(crate) struct MascotColors {
	pub base: u32,
	pub shade: u32,
	pub light: u32,
	pub ink: u32,
}
impl MascotColors {
	pub(crate) fn accessory(&self, authored: u32) -> u32 {
		color_themes::mix(authored, self.base, 18)
	}
}

/// Capture both choices before background work or a theme-keyed cache lookup.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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

	/// The palette actually displayed, which can differ from the saved dark preset.
	pub(crate) fn color_theme(self) -> ColorTheme {
		self.colors
	}

	pub(crate) fn mascot_colors(self, family: &str) -> MascotColors {
		let index = MASCOT_FAMILIES
			.iter()
			.position(|name| *name == family)
			.expect("catalog mascot family has a theme color");
		let colors =
			if self.appearance == Theme::Light { ColorTheme::Decodex } else { self.colors };
		let base = colors.palette().mascot_bases[index];
		let dark = self.hex(CANVAS);
		let light = self.hex(TEXT);
		let (dark, light) =
			if self.appearance == Theme::Light { (light, 0xffffff) } else { (dark, light) };
		let base =
			if self.appearance == Theme::Light { color_themes::mix(base, dark, 40) } else { base };
		let luminance = |color: u32| -> f32 {
			[16, 8, 0]
				.into_iter()
				.zip([0.2126, 0.7152, 0.0722])
				.map(|(shift, weight)| {
					let value = ((color >> shift) & 255) as f32 / 255.;

					weight
						* if value <= 0.04045 {
							value / 12.92
						} else {
							((value + 0.055) / 1.055).powf(2.4)
						}
				})
				.sum()
		};
		let brightness = luminance(base) + 0.05;
		let ink = if brightness / (luminance(dark) + 0.05) >= (luminance(light) + 0.05) / brightness
		{
			dark
		} else {
			light
		};

		MascotColors {
			base,
			shade: color_themes::mix(base, dark, 22),
			light: color_themes::mix(base, light, 38),
			ink,
		}
	}

	pub(crate) fn rgb(self, token: u32) -> Rgba {
		gpui::rgb(match self.appearance {
			Theme::Dark => self.colors.palette().resolve(token),
			Theme::Light => light(token),
		})
	}

	pub(crate) fn hex(self, token: u32) -> u32 {
		u32::from(self.rgb(token)) >> 8
	}

	pub(crate) fn rgba(self, token: u32) -> Rgba {
		if self.appearance == Theme::Dark
			&& let Some(material) = self.colors.palette().material(token)
		{
			return gpui::rgba(material);
		}

		let mut color = self.rgb(token >> 8);

		color.a = (token & 0xff) as f32 / 255.;

		color
	}

	/// Color themes supply the tint; native glass uses one shared opacity policy.
	pub(crate) fn material(self, token: u32, material: Material) -> Rgba {
		let color = self.rgba(token);

		if self.appearance != Theme::Dark || material != Material::LiquidGlass {
			return color;
		}

		let alpha = match token {
			SHELL_MATERIAL => 0xb0,
			SIDEBAR_MATERIAL => 0x84,
			AGENT_SIDEBAR_MATERIAL => 0x90,
			TOPBAR_MATERIAL => 0x68,
			CONTENT_MATERIAL => 0x78,
			AGENT_CHAT_OVERLAY => 0x0e,
			COMPOSER_MATERIAL => 0x88,
			SURFACE_OVERLAY_MATERIAL => 0xd8,
			_ => return color,
		};

		color.alpha(alpha as f32 / 255.)
	}

	pub(crate) fn annotation(self) -> Rgba {
		self.rgb(CANVAS).blend(self.rgb(TEXT).opacity(0.02)).opacity(0.96)
	}

	pub(crate) fn workspace(self, material: Material) -> WorkspacePalette {
		if material == Material::LiquidGlass {
			return WorkspacePalette {
				graph: gpui::rgba(0),
				conversation: self.material(AGENT_CHAT_OVERLAY, material),
			};
		}
		if self.appearance == Theme::Dark && self.colors == ColorTheme::Decodex {
			return WorkspacePalette {
				graph: self.rgb(CANVAS).opacity(0.86),
				conversation: self.rgba(CONTENT_MATERIAL),
			};
		}

		// Thin retains its matte reading planes independently of glass opacity.
		let (graph, conversation) = match self.appearance {
			Theme::Dark => (0x00000028, 0xffffff06),
			Theme::Light => (0x00000008, 0xffffffb0),
		};

		WorkspacePalette { graph: gpui::rgba(graph), conversation: gpui::rgba(conversation) }
	}
}

impl From<Theme> for ColorScheme {
	fn from(appearance: Theme) -> Self {
		Self::new(appearance, ColorTheme::Decodex)
	}
}

fn light(token: u32) -> u32 {
	match token {
		PRIMARY_FILL => 0x4766b8,
		CONTROL_ACTIVE_FILL => 0x526ca6,
		ON_PRIMARY => 0xffffff,
		INFO => 0x2e63a8,
		SUCCESS => 0x315f9d,
		WARNING => 0x8d5549,
		ERROR => 0xb43a46,
		// Materials retain their original alpha and glass ownership.
		0x0b0a0f => 0xf5f6f9,
		0x101014 | 0x100e15 => 0xeef1f6,
		0x141419 | 0x141418 | 0x111113 => 0xfafbfc,
		0x15151b | 0x17171c | 0x17151e | 0x17171a => 0xf0f2f6,
		0x222228 | 0x1d1a24 | 0x24242a | 0x242427 => 0xffffff,
		0x222c3a => 0xf0f2f7,
		0x29292d | 0x29292e | 0x27272b | 0x252830 => 0xe8edf4,
		0x302d39 | 0x30303a | 0x33333a | 0x35353c | 0x3a3a40 => 0xe0e6f0,
		0x303030 | 0x373737 | 0x464646 => 0xe2e7ef,
		0x141822 => 0xe8edf5,
		// Foreground hierarchy and neutral feedback.
		0xffffff | 0xf2f1f4 | 0xf4f2f7 | 0xeeeaf0 | 0xe7e7ea => 0x252c39,
		0xdfdee4 | 0xc5c6ce | 0xc4c4ce | 0xc0c0c5 => 0x45536a,
		0xbbb8c4 | 0xaaa4af | 0xc2bbc9 | 0xd5d5da => 0x5f6e85,
		0xb5b2bd | 0x89909c | 0x817789 => 0x657187,
		0x78818e => 0x657187,
		0x403b48 | 0x3c3744 | 0x515155 | 0x606064 => 0xb7c2d2,
		// Accent and state colors need contrast on the light material too.
		0x8baaf7 | 0x8aaaff | 0x7aa2ff | 0x6b9fff | 0x60a5fa => 0x2e63c2,
		0x22c55e | 0x77c99a => 0x42765a,
		0xe0b56f | 0xf59e0b | 0xf5c985 | 0xd4b27c => 0x996124,
		0xf97316 | 0xe49a70 | 0xffa45d | 0xff8a3d | 0xa5a0ed | 0xaaa0c4 | 0xc4a7e7 | 0xb0a0d8
		| 0xad9ae8 | 0xcac4f4 | 0xd1c2f4 => 0x2e63c2,
		0xed8585 | 0xdc2626 => 0xb43a46,
		0x94a3b8 | 0x64748b => 0x657187,
		0x9cbacb | 0x7dc4cf | 0xd5e3f1 | 0x9ec8d9 | 0xe8f3f8 | 0xf1fcff | 0x8b9fff | 0x788dff
		| 0xb0c6f3 | 0x9aafff | 0xdbe5ff | 0xc6cce0 => 0x2e63c2,
		0xd0c8d9 | 0xaaa6b8 => 0x657187,
		// Shadows, transparency and explicit content colors keep their values.
		_ => token,
	}
}

#[cfg(test)]
mod tests {
	use crate::ui_theme::{
		self, AGENT_CHAT_OVERLAY, AGENT_SIDEBAR_MATERIAL, BLUE, CANVAS, COMPOSER_MATERIAL,
		CONTENT_MATERIAL, SHELL_MATERIAL, SIDEBAR_MATERIAL, SURFACE_OVERLAY_MATERIAL, TEXT,
		TEXT_MUTED, TOPBAR_MATERIAL, appearance,
		material::Material,
		palette::{ColorScheme, ColorTheme, Theme},
	};

	#[gpui::test]
	fn popover_tints_follow_every_theme_and_keep_clear_lighter(cx: &mut gpui::TestAppContext) {
		use crate::ui_theme::{appearance::Appearance, material::GlassStyle};
		cx.update(|cx| {
			let before = (Appearance::current(), ColorTheme::configured());
			for theme in ColorTheme::ALL {
				theme.select(cx);
				for appearance in [Appearance::Dark, Appearance::Light] {
					appearance.select(cx);
					let thin = ui_theme::popover_tint(Material::Thin, GlassStyle::Regular);
					let regular =
						ui_theme::popover_tint(Material::LiquidGlass, GlassStyle::Regular);
					let clear = ui_theme::popover_tint(Material::LiquidGlass, GlassStyle::Clear);
					assert_eq!(thin, ui_theme::popover_surface());
					assert_eq!((thin.r, thin.g, thin.b), (clear.r, clear.g, clear.b));
					assert_eq!((thin.r, thin.g, thin.b), (regular.r, regular.g, regular.b));
					assert_eq!(thin.a, 1.);
					assert!(clear.a < regular.a && regular.a < thin.a);
				}
			}
			before.1.select(cx);
			before.0.select(cx);
		});
	}

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

			assert_eq!(light.rgb(TEXT), gpui::rgb(0x252c39));
			assert_eq!(light.rgb(CANVAS), gpui::rgb(0xf5f6f9));
			assert_eq!(light.rgba(SHELL_MATERIAL).a, gpui::rgba(SHELL_MATERIAL).a);
		}
	}

	#[gpui::test]
	fn a_color_snapshot_keeps_its_palette_when_the_preference_changes(
		cx: &mut gpui::TestAppContext,
	) {
		let original = ColorTheme::configured();

		appearance::with_theme(Theme::Dark, || {
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
				super::super::ACCENT,
				super::super::BLUE,
				super::super::GREEN,
				super::super::AMBER,
				super::super::ERROR,
			] {
				for background in [
					super::super::CANVAS,
					super::super::COMPOSER_MATERIAL >> 8,
					super::super::MESSAGE_MATERIAL >> 8,
				] {
					assert!(
						contrast(colors.rgb(token), colors.rgb(background)) >= 4.5,
						"{theme:?} {token:06x} on {background:06x}"
					);
				}
			}
		}
	}

	#[test]
	fn annotations_and_popovers_keep_readable_text_across_presets_and_backdrops() {
		for appearance in [Theme::Light, Theme::Dark] {
			for theme in ColorTheme::ALL {
				let colors = ColorScheme::new(appearance, theme);
				for background in
					[gpui::rgb(0), gpui::rgb(0xffffff), colors.rgb(super::super::CANVAS)]
				{
					for fill in [
						colors.annotation(),
						colors.rgb(super::super::SURFACE_OVERLAY_MATERIAL >> 8),
					] {
						assert!(
							contrast(
								colors.rgb(super::super::foreground::PRIMARY),
								background.blend(fill)
							) >= 4.5,
							"{appearance:?} {theme:?}: text on an elevated reading surface"
						);
					}
				}
			}
		}
	}

	#[test]
	fn status_text_is_readable_on_content_and_overlay_surfaces() {
		for appearance in [Theme::Dark, Theme::Light] {
			for theme in ColorTheme::ALL {
				let colors = ColorScheme::new(appearance, theme);

				for token in [
					super::super::INFO,
					super::super::SUCCESS,
					super::super::WARNING,
					super::super::ERROR,
				] {
					for background in [
						super::super::CANVAS,
						super::super::COMPOSER_MATERIAL >> 8,
						super::super::MESSAGE_MATERIAL >> 8,
						super::super::SURFACE_OVERLAY_MATERIAL >> 8,
					] {
						assert!(
							contrast(colors.rgb(token), colors.rgb(background)) >= 4.5,
							"{appearance:?} {theme:?}: status {token:06x} on {background:06x}"
						);
					}
				}
			}
		}

		let colors = ColorScheme::new(Theme::Dark, ColorTheme::Decodex);

		assert_ne!(colors.hex(super::super::SUCCESS), colors.hex(super::super::GREEN));
		assert_ne!(colors.hex(super::super::WARNING), colors.hex(super::super::AMBER));
		assert_ne!(colors.hex(super::super::ERROR), colors.hex(super::super::PRIMARY_FILL));
	}

	#[test]
	fn character_families_keep_legible_faces_and_visible_silhouettes() {
		for appearance in [Theme::Dark, Theme::Light] {
			for theme in ColorTheme::ALL {
				let colors = ColorScheme::new(appearance, theme);

				for family in super::super::MASCOT_FAMILIES {
					let character = colors.mascot_colors(family);

					assert!(
						contrast(gpui::rgb(character.ink), gpui::rgb(character.base)) >= 3.,
						"{appearance:?} {theme:?} {family}: face"
					);
					assert_ne!(character.shade, character.base);
					assert_ne!(character.light, character.base);
					assert!(
						contrast(gpui::rgb(character.base), colors.rgb(CANVAS)) >= 3.,
						"{appearance:?} {theme:?} {family}: silhouette"
					);
				}
			}
		}
	}

	#[test]
	fn selected_surfaces_keep_labels_readable() {
		for appearance in [Theme::Dark, Theme::Light] {
			for theme in ColorTheme::ALL {
				let colors = ColorScheme::new(appearance, theme);

				for token in [
					super::super::SELECTED_FILL,
					super::super::SELECTED_HOVER_FILL,
					super::super::CONTROL_SELECTED_FILL,
					super::super::CONTROL_SELECTED_HOVER,
				] {
					for background in
						[super::super::CANVAS, super::super::SURFACE_OVERLAY_MATERIAL >> 8]
					{
						let fill = colors.rgb(background).blend(colors.rgba(token));

						assert!(
							contrast(colors.rgb(TEXT), fill) >= 4.5,
							"{appearance:?} {theme:?}: label on selected surface {token:08x}"
						);
					}
				}
			}
		}
	}

	#[test]
	fn solid_primary_controls_keep_their_own_readable_foreground() {
		for appearance in [Theme::Light, Theme::Dark] {
			let colors = ColorScheme::new(appearance, ColorTheme::Decodex);

			assert_ne!(
				colors.rgb(super::super::PRIMARY_FILL),
				colors.rgb(super::super::CONTROL_ACTIVE_FILL)
			);

			for token in [super::super::PRIMARY_FILL, super::super::CONTROL_ACTIVE_FILL] {
				assert!(contrast(colors.rgb(super::super::ON_PRIMARY), colors.rgb(token)) >= 4.5);
			}
		}
		for theme in ColorTheme::ALL {
			let colors = ColorScheme::new(Theme::Dark, theme);

			for token in [super::super::PRIMARY_FILL, super::super::CONTROL_ACTIVE_FILL] {
				assert!(
					contrast(colors.rgb(super::super::ON_PRIMARY), colors.rgb(token)) >= 3.,
					"{theme:?} control icon"
				);
			}
		}
	}

	#[test]
	fn glass_transparency_is_independent_of_color_theme_and_preserves_thin() {
		// Native glass supplies the backdrop. A raw-desktop contrast calculation
		// must not replace it with an almost opaque foreground reading surface.
		let reference = ColorScheme::new(Theme::Dark, ColorTheme::TokyoNight);

		for theme in ColorTheme::ALL {
			let colors = ColorScheme::new(Theme::Dark, theme);

			for token in [
				SHELL_MATERIAL,
				SIDEBAR_MATERIAL,
				AGENT_SIDEBAR_MATERIAL,
				TOPBAR_MATERIAL,
				CONTENT_MATERIAL,
				AGENT_CHAT_OVERLAY,
				COMPOSER_MATERIAL,
				SURFACE_OVERLAY_MATERIAL,
			] {
				let glass = colors.material(token, Material::LiquidGlass);

				assert_eq!(glass.a, reference.rgba(token).a, "{theme:?} {token:08x}");
				assert_eq!(glass.alpha(1.), colors.rgba(token).alpha(1.));
				assert_eq!(colors.material(token, Material::Thin), colors.rgba(token));
			}

			let glass = colors.workspace(Material::LiquidGlass);

			assert_eq!(glass.graph.a, 0., "{theme:?} must not cover native glass");
			assert_eq!(glass.conversation.a, reference.rgba(AGENT_CHAT_OVERLAY).a);
		}
	}

	#[test]
	fn thin_workspace_keeps_its_existing_reading_planes() {
		let decodex = ColorScheme::new(Theme::Dark, ColorTheme::Decodex);
		let thin = decodex.workspace(Material::Thin);

		assert_eq!(thin.graph, decodex.rgb(CANVAS).opacity(0.86));
		assert_eq!(thin.conversation, decodex.rgba(CONTENT_MATERIAL));

		for theme in ColorTheme::ALL {
			let light = ColorScheme::new(Theme::Light, theme).workspace(Material::Thin);

			assert_eq!(light.graph, gpui::rgba(0x00000008));
			assert_eq!(light.conversation, gpui::rgba(0xffffffb0));

			if theme != ColorTheme::Decodex {
				let dark = ColorScheme::new(Theme::Dark, theme).workspace(Material::Thin);

				assert_eq!(dark.graph, gpui::rgba(0x00000028));
				assert_eq!(dark.conversation, gpui::rgba(0xffffff06));
			}
		}
	}

	#[test]
	fn theme_resolution_preserves_material_alpha_and_content_pixels() {
		appearance::with_theme(Theme::Light, || {
			assert_eq!(
				ui_theme::rgba(super::super::SHELL_MATERIAL).a,
				gpui::rgba(super::super::SHELL_MATERIAL).a
			);
			assert_eq!(ui_theme::rgba(0x00000024), gpui::rgba(0x00000024));
			assert_eq!(ui_theme::rgb(0x123456), gpui::rgb(0x123456));
			assert_eq!(ui_theme::rgb(super::super::TEXT), gpui::rgb(0x252c39));
		});
	}
}
