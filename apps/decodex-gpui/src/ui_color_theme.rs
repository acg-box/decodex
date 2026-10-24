//! Built-in desktop palettes and the host-local color preference.
use gpui::App;

#[cfg(not(test))] use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(not(test))]
static SELECTED: AtomicU8 = AtomicU8::new(u8::MAX);
#[cfg(test)]
thread_local! {
	static SELECTED: std::cell::Cell<Option<ColorTheme>> = const { std::cell::Cell::new(None) };
}

const PREFERENCE: &str = "DecodexColorTheme";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub(crate) enum ColorTheme {
	#[default]
	Decodex,
	TokyoNight,
	CatppuccinMocha,
	Dracula,
	Nord,
	GruvboxDark,
	OneDark,
	AyuDark,
	RosePine,
	Kanagawa,
	Everforest,
	SolarizedDark,
	GithubDark,
}

impl ColorTheme {
	pub(crate) const ALL: [Self; 13] = [
		Self::Decodex,
		Self::TokyoNight,
		Self::CatppuccinMocha,
		Self::Dracula,
		Self::Nord,
		Self::GruvboxDark,
		Self::OneDark,
		Self::AyuDark,
		Self::RosePine,
		Self::Kanagawa,
		Self::Everforest,
		Self::SolarizedDark,
		Self::GithubDark,
	];

	pub(crate) fn id(self) -> &'static str {
		match self {
			Self::Decodex => "decodex",
			Self::TokyoNight => "tokyo-night",
			Self::CatppuccinMocha => "catppuccin-mocha",
			Self::Dracula => "dracula",
			Self::Nord => "nord",
			Self::GruvboxDark => "gruvbox-dark",
			Self::OneDark => "one-dark",
			Self::AyuDark => "ayu-dark",
			Self::RosePine => "rose-pine",
			Self::Kanagawa => "kanagawa",
			Self::Everforest => "everforest",
			Self::SolarizedDark => "solarized-dark",
			Self::GithubDark => "github-dark",
		}
	}

	pub(crate) fn name(self) -> &'static str {
		match self {
			Self::Decodex => "Decodex",
			Self::TokyoNight => "Tokyo Night",
			Self::CatppuccinMocha => "Catppuccin Mocha",
			Self::Dracula => "Dracula",
			Self::Nord => "Nord",
			Self::GruvboxDark => "Gruvbox Dark",
			Self::OneDark => "One Dark",
			Self::AyuDark => "Ayu Dark",
			Self::RosePine => "Rosé Pine",
			Self::Kanagawa => "Kanagawa",
			Self::Everforest => "Everforest",
			Self::SolarizedDark => "Solarized Dark",
			Self::GithubDark => "GitHub Dark",
		}
	}

	pub(crate) fn from_id(id: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|theme| theme.id() == id)
	}

	pub(crate) fn configured() -> Self {
		#[cfg(not(test))]
		if let Some(theme) = Self::ALL.get(SELECTED.load(Ordering::Relaxed) as usize) {
			return *theme;
		}
		#[cfg(test)]
		if let Some(theme) = SELECTED.get() {
			return theme;
		}

		let theme = crate::ui_preferences::string(PREFERENCE, None)
			.and_then(|id| Self::from_id(&id))
			.unwrap_or_default();
		theme.cache();
		theme
	}

	fn cache(self) {
		#[cfg(not(test))]
		SELECTED.store(self as u8, Ordering::Relaxed);
		#[cfg(test)]
		SELECTED.set(Some(self));
	}

	pub(crate) fn select(self, cx: &mut App) {
		crate::ui_preferences::string(PREFERENCE, Some(self.id()));
		if self != Self::Decodex {
			super::appearance::Appearance::Dark.select(cx);
		}
		self.apply(cx);
	}

	/// Apply a palette to every window. Capture fixtures can use this without saving it.
	pub(crate) fn apply(self, cx: &mut App) {
		self.cache();
		cx.refresh_windows();
	}

	pub(crate) fn palette(self) -> &'static Palette {
		match self {
			Self::Decodex => &DECODEX,
			Self::TokyoNight => &TOKYO_NIGHT,
			Self::CatppuccinMocha => &CATPPUCCIN_MOCHA,
			Self::Dracula => &DRACULA,
			Self::Nord => &NORD,
			Self::GruvboxDark => &GRUVBOX_DARK,
			Self::OneDark => &ONE_DARK,
			Self::AyuDark => &AYU_DARK,
			Self::RosePine => &ROSE_PINE,
			Self::Kanagawa => &KANAGAWA,
			Self::Everforest => &EVERFOREST,
			Self::SolarizedDark => &SOLARIZED_DARK,
			Self::GithubDark => &GITHUB_DARK,
		}
	}
}

pub(crate) struct Palette {
	pub(crate) canvas: u32,
	pub(crate) shell_material: u32,
	pub(crate) content_material: u32,
	pub(crate) topbar_material: u32,
	pub(crate) agent_sidebar_material: u32,
	pub(crate) agent_chat_overlay: u32,
	pub(crate) sidebar_material: u32,
	pub(crate) surface_raised_material: u32,
	pub(crate) composer_material: u32,
	pub(crate) surface_overlay_material: u32,
	pub(crate) surface_hover: u32,
	pub(crate) line_strong: u32,
	pub(crate) text: u32,
	pub(crate) text_body: u32,
	pub(crate) text_muted: u32,
	pub(crate) accent: u32,
	pub(crate) blue: u32,
	pub(crate) green: u32,
	pub(crate) amber: u32,
	pub(crate) error: u32,
}

// Decodex: neutral graphite, porcelain text, and a restrained silver-blue accent.
// Surface and text ramps stay neutral; semantic colors are reserved for status.
// Stable token IDs live in ui_theme; these are the resolved display colors.
const DECODEX: Palette = Palette {
	canvas: 0x18191c,
	shell_material: 0x17181ab0,
	content_material: 0x20212478,
	topbar_material: 0x1a1c2068,
	agent_sidebar_material: 0x191b1f90,
	agent_chat_overlay: 0x23262b0e,
	sidebar_material: 0x14151884,
	surface_raised_material: 0x2c2e3446,
	composer_material: 0x2b2e3588,
	surface_overlay_material: 0x2a2d34e8,
	surface_hover: 0x353a43,
	line_strong: 0x49505b,
	text: 0xe8e8ea,
	text_body: 0xcfd0d4,
	text_muted: 0xa3a5ad,
	accent: 0x9fb6df,
	blue: 0x91b4dd,
	green: 0x97b6a2,
	amber: 0xd1b78f,
	error: 0xdf9a9f,
};

impl Palette {
	pub(super) fn material(&self, token: u32) -> Option<u32> {
		Some(match token {
			super::SHELL_MATERIAL => self.shell_material,
			super::CONTENT_MATERIAL => self.content_material,
			super::TOPBAR_MATERIAL => self.topbar_material,
			super::AGENT_SIDEBAR_MATERIAL => self.agent_sidebar_material,
			super::AGENT_CHAT_OVERLAY => self.agent_chat_overlay,
			super::SIDEBAR_MATERIAL => self.sidebar_material,
			super::SURFACE_RAISED_MATERIAL => self.surface_raised_material,
			super::COMPOSER_MATERIAL => self.composer_material,
			super::SURFACE_OVERLAY_MATERIAL => self.surface_overlay_material,
			_ => return None,
		})
	}

	/// Map the shared dark tokens without changing authored content colors.
	pub(super) fn resolve(&self, token: u32) -> u32 {
		match token {
			0x0b0a0f => self.canvas,
			0x101014 => self.shell_material >> 8,
			0x141419 | 0x141418 | 0x111113 => self.content_material >> 8,
			0x15151b => self.topbar_material >> 8,
			0x17171c | 0x17171a => self.agent_sidebar_material >> 8,
			0x100e15 => self.sidebar_material >> 8,
			0x17151e => self.surface_raised_material >> 8,
			0x1d1a24 => self.surface_overlay_material >> 8,
			0x222228 | 0x24242a | 0x242427 | 0x27272b => self.composer_material >> 8,
			0x29292d | 0x29292e | 0x252830 | 0x303030 => self.surface_raised_material >> 8,
			0x302d39 | 0x30303a | 0x33333a | 0x35353c | 0x373737 | 0x141822 =>
				self.surface_overlay_material >> 8,
			0x3a3a40 | 0x464646 => self.surface_hover,
			0xffffff | 0xf2f1f4 | 0xf4f2f7 | 0xeeeaf0 | 0xe7e7ea => self.text,
			0xdfdee4 | 0xc5c6ce | 0xc4c4ce | 0xc0c0c5 => self.text_body,
			0xbbb8c4 | 0xaaa4af | 0xc2bbc9 | 0xd5d5da | 0xb5b2bd | 0x89909c | 0x817789
			| 0x78818e | 0x94a3b8 | 0x64748b | 0xd0c8d9 | 0xaaa6b8 => self.text_muted,
			0x403b48 | 0x3c3744 | 0x515155 | 0x606064 => self.line_strong,
			0x8baaf7 | 0x8aaaff | 0x7aa2ff | 0x6b9fff | 0x60a5fa | 0x9cbacb | 0x7dc4cf
			| 0xd5e3f1 | 0x9ec8d9 | 0xe8f3f8 | 0xf1fcff | 0x8b9fff | 0x788dff | 0xb0c6f3
			| 0x9aafff | 0xdbe5ff | 0xc6cce0 => self.blue,
			0x22c55e | 0x77c99a => self.green,
			0xe0b56f | 0xf59e0b | 0xf5c985 | 0xd4b27c => self.amber,
			0xf97316 | 0xe49a70 | 0xffa45d | 0xff8a3d | 0xa5a0ed | 0xaaa0c4 | 0xc4a7e7
			| 0xb0a0d8 | 0xad9ae8 | 0xcac4f4 | 0xd1c2f4 => self.accent,
			0xed8585 | 0xdc2626 | 0xef4444 => self.error,
			_ => token,
		}
	}

	// Backgrounds: canvas, sidebar, raised. Foregrounds: text, muted, border.
	// Accents: primary, link, success, warning, error. Keep native glass translucent.
	const fn dark(backgrounds: [u32; 3], foregrounds: [u32; 3], accents: [u32; 5]) -> Self {
		let [canvas, sidebar, raised] = backgrounds;
		let [text, muted, border] = foregrounds;
		let [accent, blue, green, amber, error] = accents;
		Self {
			canvas,
			shell_material: (canvas << 8) | 0xb0,
			content_material: (canvas << 8) | 0x78,
			topbar_material: (sidebar << 8) | 0x68,
			agent_sidebar_material: (sidebar << 8) | 0x90,
			agent_chat_overlay: (canvas << 8) | 0x0e,
			sidebar_material: (sidebar << 8) | 0x84,
			surface_raised_material: (raised << 8) | 0x46,
			composer_material: (raised << 8) | 0x88,
			surface_overlay_material: (raised << 8) | 0xd8,
			surface_hover: raised,
			line_strong: border,
			text,
			text_body: text,
			text_muted: muted,
			accent,
			blue,
			green,
			amber,
			error,
		}
	}
}

// Palette sources; material alpha and semantic role assignment are Decodex-specific.
// https://github.com/folke/tokyonight.nvim/blob/main/extras/lua/tokyonight_night.lua
const TOKYO_NIGHT: Palette = Palette::dark(
	[0x1a1b26, 0x16161e, 0x292e42],
	[0xc0caf5, 0xa9b1d6, 0x414868],
	[0xbb9af7, 0x7aa2f7, 0x9ece6a, 0xe0af68, 0xf7768e],
);
// https://github.com/catppuccin/palette/blob/main/palette.json (Mocha)
const CATPPUCCIN_MOCHA: Palette = Palette::dark(
	[0x1e1e2e, 0x181825, 0x313244],
	[0xcdd6f4, 0xa6adc8, 0x45475a],
	[0xcba6f7, 0x89b4fa, 0xa6e3a1, 0xf9e2af, 0xf38ba8],
);
// https://github.com/dracula/dracula-theme#color-palette-oss
const DRACULA: Palette = Palette::dark(
	[0x282a36, 0x21222c, 0x44475a],
	[0xf8f8f2, 0xbfc1d6, 0x6272a4],
	[0xbd93f9, 0x8be9fd, 0x50fa7b, 0xf1fa8c, 0xff5555],
);
// https://github.com/nordtheme/nord/blob/develop/src/nord.css
const NORD: Palette = Palette::dark(
	[0x2e3440, 0x2e3440, 0x3b4252],
	[0xeceff4, 0xd8dee9, 0x4c566a],
	[0x88c0d0, 0x81a1c1, 0xa3be8c, 0xebcb8b, 0xbf616a],
);
// https://github.com/morhetz/gruvbox/blob/master/colors/gruvbox.vim
const GRUVBOX_DARK: Palette = Palette::dark(
	[0x282828, 0x1d2021, 0x3c3836],
	[0xebdbb2, 0xbdae93, 0x665c54],
	[0xfe8019, 0x83a598, 0xb8bb26, 0xfabd2f, 0xfb4934],
);
// https://github.com/atom/one-dark-syntax/blob/master/styles/colors.less
const ONE_DARK: Palette = Palette::dark(
	[0x282c34, 0x21252b, 0x323842],
	[0xabb2bf, 0x9da5b4, 0x4b5263],
	[0xc678dd, 0x61afef, 0x98c379, 0xe5c07b, 0xe06c75],
);

// https://github.com/ayu-theme/ayu-colors/blob/master/themes/dark.yaml
// Secondary text is brighter than the original comments to preserve UI readability.
const AYU_DARK: Palette = Palette::dark(
	[0x10141c, 0x0d1017, 0x141821],
	[0xbfbdb6, 0x8a9199, 0x3e4b59],
	[0xe6b450, 0x59c2ff, 0xaad94c, 0xffb454, 0xf07178],
);
// https://github.com/rose-pine/neovim/blob/main/lua/rose-pine/palette.lua
const ROSE_PINE: Palette = Palette::dark(
	[0x191724, 0x1f1d2e, 0x26233a],
	[0xe0def4, 0x908caa, 0x524f67],
	[0xc4a7e7, 0x9ccfd8, 0x95b1ac, 0xf6c177, 0xeb6f92],
);
// https://github.com/rebelot/kanagawa.nvim/blob/master/lua/kanagawa/colors.lua (Wave)
const KANAGAWA: Palette = Palette::dark(
	[0x1f1f28, 0x16161d, 0x2a2a37],
	[0xdcd7ba, 0xc8c093, 0x54546d],
	[0x957fb8, 0x7e9cd8, 0x98bb6c, 0xe6c384, 0xff5d62],
);
// https://github.com/sainnhe/everforest/blob/master/autoload/everforest.vim (Dark hard)
const EVERFOREST: Palette = Palette::dark(
	[0x272e33, 0x1e2326, 0x2e383c],
	[0xd3c6aa, 0x9da9a0, 0x4f5b58],
	[0xa7c080, 0x7fbbb3, 0xa7c080, 0xdbbc7f, 0xe67e80],
);
// https://ethanschoonover.com/solarized/#the-values
// Use the lighter neutral tones for UI text on raised surfaces.
const SOLARIZED_DARK: Palette = Palette::dark(
	[0x002b36, 0x002b36, 0x073642],
	[0xeee8d5, 0x93a1a1, 0x586e75],
	[0x2aa198, 0x268bd2, 0x859900, 0xb58900, 0xdc322f],
);
// https://github.com/primer/github-vscode-theme/blob/main/src/colors.js
// Uses @primer/primitives 7.10.0, with the theme's foreground override.
const GITHUB_DARK: Palette = Palette::dark(
	[0x0d1117, 0x010409, 0x161b22],
	[0xe6edf3, 0x8b949e, 0x30363d],
	[0xa371f7, 0x58a6ff, 0x3fb950, 0xd29922, 0xf85149],
);

#[cfg(test)]
mod tests {
	use super::ColorTheme;

	#[test]
	fn preference_ids_round_trip_and_unknown_ids_are_rejected() {
		for theme in ColorTheme::ALL {
			assert_eq!(ColorTheme::from_id(theme.id()), Some(theme));
			assert_eq!(ColorTheme::ALL.iter().filter(|other| other.id() == theme.id()).count(), 1);
		}
		assert_eq!(ColorTheme::from_id("removed-theme"), None);
	}

	#[test]
	fn every_palette_keeps_body_and_secondary_text_readable() {
		fn luminance(rgb: u32) -> f64 {
			[16, 8, 0]
				.into_iter()
				.zip([0.2126, 0.7152, 0.0722])
				.map(|(shift, weight)| {
					let channel = f64::from((rgb >> shift) & 0xff) / 255.;
					weight
						* if channel <= 0.04045 {
							channel / 12.92
						} else {
							((channel + 0.055) / 1.055).powf(2.4)
						}
				})
				.sum()
		}
		for theme in ColorTheme::ALL {
			let palette = theme.palette();
			for background in [
				palette.canvas,
				palette.composer_material >> 8,
				palette.surface_overlay_material >> 8,
				palette.surface_hover,
			] {
				for text in [palette.text, palette.text_body, palette.text_muted] {
					let contrast = (luminance(text) + 0.05) / (luminance(background) + 0.05);
					assert!(
						contrast >= 4.5,
						"{}: {text:06x} on {background:06x}: {contrast}",
						theme.name()
					);
				}
			}
		}
	}
}
