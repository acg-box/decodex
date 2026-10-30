//! Independent window, input-bar and popover appearance preferences.
use std::{
	env,
	sync::atomic::{AtomicU8, Ordering},
};

use gpui::App;

use crate::{ui_preferences, ui_theme::window_material};

static MATERIALS: [AtomicU8; 3] =
	[AtomicU8::new(u8::MAX), AtomicU8::new(u8::MAX), AtomicU8::new(u8::MAX)];
static GLASS_STYLES: [AtomicU8; 2] = [AtomicU8::new(u8::MAX), AtomicU8::new(u8::MAX)];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum Material {
	Thin,
	LiquidGlass,
}
impl Material {
	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::Thin => "Thin",
			Self::LiquidGlass => "Liquid Glass",
		}
	}

	fn preference(self) -> &'static str {
		match self {
			Self::Thin => "thin",
			Self::LiquidGlass => "liquid-glass",
		}
	}

	fn from_preference(value: &str) -> Option<Self> {
		match value {
			"thin" => Some(Self::Thin),
			"liquid-glass" => Some(Self::LiquidGlass),
			_ => None,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub(crate) enum Surface {
	Window,
	InputBar,
	Popover,
}
impl Surface {
	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::Window => "Window material",
			Self::InputBar => "Input bar material",
			Self::Popover => "Popover material",
		}
	}

	fn key(self) -> &'static str {
		match self {
			Self::Window => "windowMaterial",
			Self::InputBar => "inputBarMaterial",
			Self::Popover => "popoverMaterial",
		}
	}

	pub(crate) fn material(self) -> Material {
		let cached = MATERIALS[self as usize].load(Ordering::Relaxed);

		if cached != u8::MAX {
			return if cached == 0 { Material::Thin } else { Material::LiquidGlass };
		}

		let saved = ui_preferences::string(self.key(), None);
		// Follow the window until the user chooses a separate popover material.
		if self == Self::Popover && saved.is_none() {
			return Self::Window.material();
		}
		let material = initial_material(saved.as_deref());

		MATERIALS[self as usize].store(material as u8, Ordering::Relaxed);

		material
	}

	pub(crate) fn select(self, material: Material, cx: &mut App) {
		ui_preferences::string(self.key(), Some(material.preference()));

		MATERIALS[self as usize].store(material as u8, Ordering::Relaxed);

		refresh(self, cx);
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum GlassStyle {
	#[default]
	Regular,
	Clear,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub(crate) enum GlassScope {
	Window,
	Popover,
}
impl GlassScope {
	pub(crate) const fn id(self) -> &'static str {
		match self {
			Self::Window => "window-glass",
			Self::Popover => "popover-glass",
		}
	}

	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::Window => "Window glass style",
			Self::Popover => "Popover glass style",
		}
	}

	fn key(self) -> &'static str {
		match self {
			Self::Window => "DecodexGlassClear",
			Self::Popover => "DecodexPopoverGlassClear",
		}
	}

	pub(crate) fn configured(self) -> GlassStyle {
		let cache = &GLASS_STYLES[self as usize];

		if self == Self::Window
			&& cache.load(Ordering::Relaxed) == u8::MAX
			&& let Ok(value) = env::var("DECODEX_GLASS_STYLE")
		{
			cache.store(u8::from(value == "clear"), Ordering::Relaxed);
		}
		if ui_preferences::boolean(self.key(), cache, None, false) {
			GlassStyle::Clear
		} else {
			GlassStyle::Regular
		}
	}

	pub(crate) fn select(self, style: GlassStyle, cx: &mut App) {
		ui_preferences::boolean(
			self.key(),
			&GLASS_STYLES[self as usize],
			Some(style == GlassStyle::Clear),
			false,
		);

		refresh(Surface::Window, cx);
	}
}

fn initial_material(saved: Option<&str>) -> Material {
	saved.and_then(Material::from_preference).unwrap_or(Material::LiquidGlass)
}

fn refresh(surface: Surface, cx: &mut App) {
	cx.defer(move |cx| {
		if surface == Surface::Window {
			for handle in cx.windows() {
				let _ = handle.update(cx, |_, window, _| window_material::configure(window));
			}
		}

		// Attached input bars and popup panels update their own native surfaces
		// during this refresh, keeping their existing editor and keyboard focus.
		cx.refresh_windows();
	});
}

#[cfg(test)]
mod tests {
	use crate::ui_theme::material::{GlassScope, GlassStyle, Material, Surface};

	#[test]
	fn material_names_round_trip_and_match_the_native_panel() {
		for material in [Material::Thin, Material::LiquidGlass] {
			assert_eq!(Material::from_preference(material.preference()), Some(material));
		}

		assert_eq!(Material::from_preference("plain"), None);
	}

	#[gpui::test]
	fn material_preferences_are_independent(cx: &mut gpui::TestAppContext) {
		cx.update(|cx| {
			Surface::Window.select(Material::Thin, cx);
			Surface::InputBar.select(Material::LiquidGlass, cx);
			Surface::Popover.select(Material::LiquidGlass, cx);

			assert_eq!(Surface::Window.material(), Material::Thin);
			assert_eq!(Surface::Popover.material(), Material::LiquidGlass);

			Surface::InputBar.select(Material::Thin, cx);

			assert_eq!(Surface::Window.material(), Material::Thin);

			Surface::Window.select(Material::LiquidGlass, cx);

			assert_eq!(Surface::InputBar.material(), Material::Thin);
			Surface::Popover.select(Material::Thin, cx);
			assert_eq!(Surface::Window.material(), Material::LiquidGlass);

			Surface::InputBar.select(Material::LiquidGlass, cx);
		});
	}

	#[gpui::test]
	fn window_and_popover_styles_do_not_change_each_other_or_the_input(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(|cx| {
			Surface::Window.select(Material::Thin, cx);
			Surface::InputBar.select(Material::Thin, cx);
			Surface::Popover.select(Material::LiquidGlass, cx);
			GlassScope::Window.select(GlassStyle::Regular, cx);
			GlassScope::Popover.select(GlassStyle::Clear, cx);
			assert_eq!(GlassScope::Window.configured(), GlassStyle::Regular);
			assert_eq!(GlassScope::Popover.configured(), GlassStyle::Clear);
			GlassScope::Window.select(GlassStyle::Clear, cx);
			GlassScope::Popover.select(GlassStyle::Regular, cx);
			assert_eq!(GlassScope::Window.configured(), GlassStyle::Clear);
			assert_eq!(GlassScope::Popover.configured(), GlassStyle::Regular);
			assert_eq!(Surface::Window.material(), Material::Thin);
			assert_eq!(Surface::InputBar.material(), Material::Thin);
			assert_eq!(Surface::Popover.material(), Material::LiquidGlass);
		});
	}
}
