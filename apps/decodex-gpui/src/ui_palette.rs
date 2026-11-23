//! Paired app colors. The existing RGB values are stable color tokens, not
//! content pixels. Resolve them here so charts, text, controls and materials
//! share a palette. Keep media and authored diagram colors outside this path.
use gpui::Rgba;

use crate::ui_theme::{
	AGENT_CHAT_OVERLAY, AGENT_SIDEBAR_MATERIAL, CANVAS, COMPOSER_MATERIAL, CONTENT_MATERIAL,
	ColorTheme, SHELL_MATERIAL, SIDEBAR_MATERIAL, SURFACE_OVERLAY_MATERIAL, TEXT, TOPBAR_MATERIAL,
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
	pub(crate) fn is_light(self) -> bool {
		self.appearance == Theme::Light
	}

	pub(crate) const fn new(appearance: Theme, colors: ColorTheme) -> Self {
		Self { appearance, colors }
	}

	pub(crate) fn current() -> Self {
		Self::new(Theme::current(), ColorTheme::configured())
	}

	pub(super) fn palette(self) -> &'static color_themes::Palette {
		self.colors.palette(self.appearance)
	}

	pub(crate) fn color_theme(self) -> ColorTheme {
		self.colors
	}

	/// Loading companions retain their approved brand palette in both appearances.
	pub(crate) fn loading_mascot_colors(self, receiver: bool) -> MascotColors {
		let [base, light, shade, ink] = match (self.is_light(), receiver) {
			(true, false) => [0x79b0cf, 0xc1dfef, 0x4386af, 0x172332],
			(false, false) => [0x9bdbfa, 0xe0f6ff, 0x4caee6, 0x172332],
			(true, true) => [0x8b95d6, 0xd8def8, 0x636fad, 0x171c31],
			(false, true) => [0xaeb5fc, 0xe0e3ff, 0x7f88e7, 0x171c31],
		};
		MascotColors { base, light, shade, ink }
	}

	pub(crate) fn mascot_colors(self, family: &str) -> MascotColors {
		let index = MASCOT_FAMILIES
			.iter()
			.position(|name| *name == family)
			.expect("catalog mascot family has a theme color");
		let palette = self.palette();
		let base = palette.mascot_bases[index];
		let (dark, light) =
			if self.is_light() { (palette.text, 0xffffff) } else { (palette.canvas, palette.text) };
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
		gpui::rgb(self.palette().resolve(token))
	}

	pub(crate) fn hex(self, token: u32) -> u32 {
		u32::from(self.rgb(token)) >> 8
	}

	pub(crate) fn rgba(self, token: u32) -> Rgba {
		if let Some(material) = self.palette().material(token) {
			return gpui::rgba(material);
		}

		let mut color = self.rgb(token >> 8);

		color.a = (token & 0xff) as f32 / 255.;

		color
	}

	/// Color themes supply the tint; native glass uses one shared opacity policy.
	pub(crate) fn material(self, token: u32, material: Material) -> Rgba {
		let color = self.rgba(token);

		if material != Material::LiquidGlass {
			return color;
		}

		let dark_alpha = match token {
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

		let alpha = if self.is_light() { token & 0xff } else { dark_alpha };
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
		if self.appearance == Theme::Dark && self.color_theme() == ColorTheme::Decodex {
			return WorkspacePalette {
				graph: self.rgb(CANVAS).opacity(0.86),
				conversation: self.rgba(CONTENT_MATERIAL),
			};
		}

		// Thin retains its matte reading planes independently of glass opacity.
		match self.appearance {
			Theme::Dark => WorkspacePalette {
				graph: gpui::rgba(0x00000028),
				conversation: gpui::rgba(0xffffff06),
			},
			Theme::Light => WorkspacePalette {
				graph: gpui::rgba(0x00000008),
				conversation: self.rgb(SURFACE_OVERLAY_MATERIAL >> 8).opacity(0xb0 as f32 / 255.),
			},
		}
	}
}

#[cfg(test)]
impl From<Theme> for ColorScheme {
	fn from(appearance: Theme) -> Self {
		Self::new(appearance, ColorTheme::Decodex)
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
	fn overlay_tints_follow_every_theme_and_leave_clear_untinted(cx: &mut gpui::TestAppContext) {
		use crate::ui_theme::{appearance::Appearance, material::GlassStyle};
		cx.update(|cx| {
			let before = (Appearance::current(), ColorTheme::configured());
			for theme in ColorTheme::ALL {
				theme.select(cx);
				for appearance in [Appearance::Dark, Appearance::Light] {
					appearance.select(cx);
					let thin = ui_theme::popover_tint(Material::Thin, GlassStyle::Regular).unwrap();
					let regular =
						ui_theme::popover_tint(Material::LiquidGlass, GlassStyle::Regular).unwrap();
					let clear = ui_theme::popover_tint(Material::LiquidGlass, GlassStyle::Clear);
					assert_eq!(thin, ui_theme::popover_surface());
					assert_eq!(clear, None);
					assert_eq!((thin.r, thin.g, thin.b), (regular.r, regular.g, regular.b));
					assert_eq!(thin.a, 1.);
					assert!(regular.a < thin.a);
				}
			}
			before.1.select(cx);
			before.0.select(cx);
		});
	}

	#[test]
	fn presets_resolve_their_own_roles_in_both_appearances() {
		for appearance in [Theme::Light, Theme::Dark] {
			for theme in ColorTheme::ALL {
				let palette = theme.palette(appearance);
				let colors = ColorScheme::new(appearance, theme);
				for (token, expected) in [
					(CANVAS, palette.canvas),
					(TEXT, palette.text),
					(TEXT_MUTED, palette.text_muted),
					(BLUE, palette.blue),
				] {
					assert_eq!(colors.hex(token), expected, "{appearance:?}/{theme:?}");
				}
				assert_eq!(colors.rgba(SHELL_MATERIAL), gpui::rgba(palette.shell_material));
				assert_eq!(colors.rgba(AGENT_CHAT_OVERLAY), gpui::rgba(palette.agent_chat_overlay));
				assert_eq!(colors.rgb(0x123456), gpui::rgb(0x123456));
				assert_eq!(colors.rgba(0x12345678), gpui::rgba(0x12345678));
			}
		}
	}

	#[gpui::test]
	fn a_color_snapshot_keeps_its_palette_when_the_preference_changes(
		cx: &mut gpui::TestAppContext,
	) {
		let original = ColorTheme::configured();

		for theme in [Theme::Light, Theme::Dark] {
			appearance::with_theme(theme, || {
				cx.update(|cx| ColorTheme::TokyoNight.apply(cx));

				let captured = ColorScheme::current();

				cx.update(|cx| ColorTheme::Nord.apply(cx));

				assert_ne!(captured, ColorScheme::current());
				assert_eq!(captured.hex(TEXT), ColorTheme::TokyoNight.palette(theme).text);
			});
		}

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
	fn decodex_reading_hierarchy_and_accents_have_contrast_in_both_appearances() {
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
	fn reading_text_has_contrast_in_every_palette() {
		for appearance in [Theme::Light, Theme::Dark] {
			for theme in ColorTheme::ALL {
				let colors = ColorScheme::new(appearance, theme);
				for token in [TEXT, super::super::foreground::BODY, TEXT_MUTED] {
					for background in
						[CANVAS, COMPOSER_MATERIAL >> 8, SURFACE_OVERLAY_MATERIAL >> 8]
					{
						assert!(
							contrast(colors.rgb(token), colors.rgb(background)) >= 4.5,
							"{appearance:?} {theme:?}: text {token:06x} on {background:06x}"
						);
					}
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
				let states = [
					super::super::INFO,
					super::super::SUCCESS,
					super::super::WARNING,
					super::super::ERROR,
				];
				for (index, token) in states.iter().enumerate() {
					let a = colors.rgb(*token);
					for other in &states[index + 1..] {
						let b = colors.rgb(*other);
						let distance =
							((a.r - b.r).powi(2) + (a.g - b.g).powi(2) + (a.b - b.b).powi(2))
								.sqrt();
						assert!(
							distance >= 0.08,
							"{appearance:?} {theme:?}: status colors are too similar"
						);
					}
				}

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
		for appearance in [Theme::Light, Theme::Dark] {
			for theme in ColorTheme::ALL {
				let colors = ColorScheme::new(appearance, theme);
				for token in [super::super::PRIMARY_FILL, super::super::CONTROL_ACTIVE_FILL] {
					assert!(
						contrast(colors.rgb(super::super::ON_PRIMARY), colors.rgb(token)) >= 3.,
						"{appearance:?} {theme:?} control icon"
					);
				}
			}
		}
	}

	#[test]
	fn glass_transparency_is_independent_of_color_theme_and_preserves_thin() {
		// Native glass supplies the backdrop. A raw-desktop contrast calculation
		// must not replace it with an almost opaque foreground reading surface.
		for (appearance, reference) in
			[(Theme::Dark, ColorTheme::TokyoNight), (Theme::Light, ColorTheme::Decodex)]
		{
			let reference = ColorScheme::new(appearance, reference);

			for theme in ColorTheme::ALL {
				let colors = ColorScheme::new(appearance, theme);

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
			assert_eq!(
				light.conversation,
				gpui::rgb(theme.palette(Theme::Light).surface_overlay_material >> 8)
					.opacity(0xb0 as f32 / 255.)
			);

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
				gpui::rgba(ColorTheme::configured().palette(Theme::Light).shell_material).a
			);
			assert_eq!(ui_theme::rgba(0x00000024), gpui::rgba(0x00000024));
			assert_eq!(ui_theme::rgb(0x123456), gpui::rgb(0x123456));
			assert_eq!(
				ui_theme::rgb(super::super::TEXT),
				gpui::rgb(ColorTheme::configured().palette(Theme::Light).text)
			);
		});
	}
}
