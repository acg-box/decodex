//! One host-local appearance choice for GPUI and AppKit windows.
use std::cell::Cell;

use gpui::{App, Context, Window, WindowAppearance};

use crate::{ui_preferences, ui_theme::ColorTheme};

// GPUI paints on the app thread. Background jobs receive an explicit Theme,
// so a theme change cannot alter an image while a worker prepares it.
thread_local! {
	static STATE: Cell<(Appearance, Theme)> = const { Cell::new((Appearance::System, Theme::Dark)) };
}

const KEY: &str = "appearance";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Appearance {
	#[default]
	System,
	Light,
	Dark,
}
impl Appearance {
	pub(crate) fn current() -> Self {
		STATE.get().0
	}

	fn from_preference(value: Option<&str>, colors: ColorTheme) -> Self {
		match value {
			Some("light") => Self::Light,
			Some("dark") => Self::Dark,
			// Existing dark presets predate the separate appearance preference.
			None if colors != ColorTheme::Decodex => Self::Dark,
			_ => Self::System,
		}
	}

	fn preference(self) -> &'static str {
		match self {
			Self::System => "system",
			Self::Light => "light",
			Self::Dark => "dark",
		}
	}

	fn native(self) -> Option<WindowAppearance> {
		match self {
			Self::System => None,
			Self::Light => Some(WindowAppearance::Light),
			Self::Dark => Some(WindowAppearance::Dark),
		}
	}

	fn resolve(self, appearance: WindowAppearance) -> Theme {
		match self.native().unwrap_or(appearance) {
			WindowAppearance::Light | WindowAppearance::VibrantLight => Theme::Light,
			WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::Dark,
		}
	}

	pub(crate) fn select(self, cx: &mut App) {
		ui_preferences::string(KEY, Some(self.preference()));

		self.apply(cx);
	}

	/// Apply appearance without changing the saved preference.
	pub(crate) fn apply(self, cx: &mut App) {
		STATE.set((self, Theme::current()));
		// None resumes system tracking. This also changes the native glass and
		// menu-bar panels; no per-window AppKit override is needed.
		cx.set_window_appearance(self.native());

		synchronize(cx.window_appearance(), cx);

		cx.refresh_windows();
	}
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Theme {
	Light,
	Dark,
}
impl Theme {
	pub(crate) fn current() -> Self {
		STATE.get().1
	}
}

pub(crate) fn initialize(cx: &mut App) {
	let value = ui_preferences::string(KEY, None);
	let appearance = Appearance::from_preference(value.as_deref(), ColorTheme::configured());

	STATE.set((appearance, Theme::current()));
	cx.set_window_appearance(appearance.native());

	synchronize(cx.window_appearance(), cx);
}

pub(crate) fn observe<T>(window: &mut Window, cx: &mut Context<T>)
where
	T: 'static,
{
	synchronize(window.appearance(), cx);

	cx.observe_window_appearance(window, |_, window, cx| {
		synchronize(window.appearance(), cx);

		cx.notify();
	})
	.detach();
}

#[cfg(test)]
pub(crate) fn with_theme<T>(theme: Theme, f: impl FnOnce() -> T) -> T {
	struct Restore((Appearance, Theme));

	impl Drop for Restore {
		fn drop(&mut self) {
			STATE.set(self.0);
		}
	}

	let _restore = Restore(STATE.get());

	STATE.set((Appearance::current(), theme));

	f()
}

fn synchronize(appearance: WindowAppearance, cx: &mut App) {
	let choice = Appearance::current();
	let theme = choice.resolve(appearance);

	if theme != Theme::current() {
		STATE.set((choice, theme));
		// GPUI refresh invalidates cached child views, including inactive windows.
		cx.refresh_windows();
	}
}

#[cfg(test)]
mod tests {
	use std::rc::Rc;

	use gpui::{AppContext as _, Entity, IntoElement, Render};

	use crate::ui_theme::appearance::{
		self, Appearance, Cell, ColorTheme, Context, STATE, Theme, Window, WindowAppearance,
	};

	struct Sample(Rc<Cell<Theme>>);
	impl Render for Sample {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			self.0.set(Theme::current());

			gpui::div()
		}
	}

	struct Root(Entity<Sample>);
	impl Render for Root {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			self.0.clone()
		}
	}

	#[test]
	fn preferences_round_trip_and_unknown_values_follow_system() {
		for colors in ColorTheme::ALL {
			for choice in [Appearance::System, Appearance::Light, Appearance::Dark] {
				assert_eq!(Appearance::from_preference(Some(choice.preference()), colors), choice);
			}

			let initial =
				if colors == ColorTheme::Decodex { Appearance::System } else { Appearance::Dark };

			assert_eq!(Appearance::from_preference(None, colors), initial);
			assert_eq!(Appearance::from_preference(Some("obsolete"), colors), Appearance::System);
		}
	}

	#[test]
	fn explicit_choice_wins_and_system_follows_both_vibrant_variants() {
		for native in [WindowAppearance::Light, WindowAppearance::VibrantLight] {
			assert_eq!(Appearance::System.resolve(native), Theme::Light);
			assert_eq!(Appearance::Dark.resolve(native), Theme::Dark);
		}
		for native in [WindowAppearance::Dark, WindowAppearance::VibrantDark] {
			assert_eq!(Appearance::System.resolve(native), Theme::Dark);
			assert_eq!(Appearance::Light.resolve(native), Theme::Light);
		}
	}

	#[gpui::test]
	fn system_changes_refresh_cached_children_and_other_windows(cx: &mut gpui::TestAppContext) {
		let before = STATE.get();
		let mut windows = Vec::new();
		let mut samples = Vec::new();

		for _ in 0..2 {
			let sample = Rc::new(Cell::new(Theme::Dark));

			samples.push(sample.clone());
			windows.push(cx.add_window(move |window, cx| {
				appearance::observe(window, cx);

				Root(cx.new(|_| Sample(sample)))
			}));
		}

		cx.run_until_parked();
		cx.update(|cx| Appearance::System.select(cx));

		for (native, expected) in
			[(WindowAppearance::Dark, Theme::Dark), (WindowAppearance::Light, Theme::Light)]
		{
			cx.update(|cx| appearance::synchronize(native, cx));
			cx.run_until_parked();

			for window in &windows {
				cx.update_window((*window).into(), |_, window, cx| window.draw(cx).clear(cx))
					.unwrap();
			}

			assert!(samples.iter().all(|sample| sample.get() == expected));
		}

		cx.update(|cx| Appearance::Dark.select(cx));
		cx.update(|cx| appearance::synchronize(WindowAppearance::Light, cx));
		cx.run_until_parked();

		assert_eq!(Theme::current(), Theme::Dark, "explicit choice must ignore OS changes");

		STATE.set(before);
	}
}
