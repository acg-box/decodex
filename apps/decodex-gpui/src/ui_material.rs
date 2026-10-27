//! Independent material preferences for windows and the input bar.
use std::{
	env,
	sync::atomic::{AtomicU8, Ordering},
};

use gpui::App;

use crate::{ui_preferences, ui_theme::window_material};

static MATERIALS: [AtomicU8; 2] = [AtomicU8::new(u8::MAX), AtomicU8::new(u8::MAX)];
static GLASS_STYLE: AtomicU8 = AtomicU8::new(u8::MAX);

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
}
impl Surface {
	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::Window => "Window material",
			Self::InputBar => "Input bar material",
		}
	}

	fn key(self) -> &'static str {
		match self {
			Self::Window => "windowMaterial",
			Self::InputBar => "inputBarMaterial",
		}
	}

	pub(crate) fn material(self) -> Material {
		let cached = MATERIALS[self as usize].load(Ordering::Relaxed);

		if cached != u8::MAX {
			return if cached == 0 { Material::Thin } else { Material::LiquidGlass };
		}

		let saved = ui_preferences::string(self.key(), None);
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
impl GlassStyle {
	pub(crate) fn configured() -> Self {
		if GLASS_STYLE.load(Ordering::Relaxed) == u8::MAX
			&& let Ok(value) = env::var("DECODEX_GLASS_STYLE")
		{
			GLASS_STYLE.store(u8::from(value == "clear"), Ordering::Relaxed);
		}
		if ui_preferences::boolean("DecodexGlassClear", &GLASS_STYLE, None, false) {
			Self::Clear
		} else {
			Self::Regular
		}
	}

	pub(crate) fn select(self, cx: &mut App) {
		ui_preferences::boolean(
			"DecodexGlassClear",
			&GLASS_STYLE,
			Some(self == Self::Clear),
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
	use crate::ui_theme::material::{Material, Surface};

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

			assert_eq!(Surface::Window.material(), Material::Thin);

			Surface::InputBar.select(Material::Thin, cx);

			assert_eq!(Surface::Window.material(), Material::Thin);

			Surface::Window.select(Material::LiquidGlass, cx);

			assert_eq!(Surface::InputBar.material(), Material::Thin);

			Surface::InputBar.select(Material::LiquidGlass, cx);
		});
	}
}
