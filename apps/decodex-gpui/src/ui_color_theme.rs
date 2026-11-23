//! Built-in desktop palettes and the host-local color preference.
#[cfg(not(test))] use std::sync::atomic::{AtomicU8, Ordering};

use gpui::App;

use crate::{
	ui_preferences,
	ui_theme::{
		AGENT_CHAT_OVERLAY, AGENT_SIDEBAR_MATERIAL, COMPOSER_MATERIAL, CONTENT_MATERIAL,
		CONTROL_ACTIVE_FILL, CONTROL_SELECTED_BORDER, CONTROL_SELECTED_FILL,
		CONTROL_SELECTED_HOVER, ERROR, INFO, MESSAGE_MATERIAL, ON_PRIMARY, PRIMARY_FILL,
		SELECTED_BORDER, SELECTED_FILL, SELECTED_HOVER_FILL, SHELL_MATERIAL, SIDEBAR_MATERIAL,
		SUCCESS, SURFACE_OVERLAY_MATERIAL, SURFACE_RAISED_MATERIAL, TOPBAR_MATERIAL, WARNING,
		appearance::Theme,
	},
};

#[cfg(test)]
thread_local! {
	static SELECTED: std::cell::Cell<Option<ColorTheme>> = const { std::cell::Cell::new(None) };
}

const PREFERENCE: &str = "DecodexColorTheme";
// Decodex uses one low-chroma blue scale from the frame to the reading surfaces.
// Brighter blues mark actions; nearby blue-gray steps distinguish content and controls.
// Solid controls have a separate foreground; text accents do not set button brightness.
// Stable token IDs live in ui_theme; these are the resolved display colors.
const DECODEX_DARK: Palette = Palette {
	canvas: 0x141a24,
	shell_material: 0x181e29f8,
	content_material: 0x202733f0,
	topbar_material: 0x1c2431f0,
	agent_sidebar_material: 0x19212df0,
	agent_chat_overlay: 0x202733db,
	sidebar_material: 0x151c27f8,
	surface_raised_material: 0x272f3ea0,
	composer_material: 0x2a3342f8,
	message_material: 0x2d3748b8,
	surface_overlay_material: 0x2b3443fa,
	surface_hover: 0x333c4d,
	selection_fill: 0x2e4568e8,
	selection_hover: 0x39567aed,
	selection_border: 0x85aff780,
	control_selected_fill: 0x9faec638,
	control_selected_hover: 0xaabbd24a,
	control_selected_border: 0xb1c2db40,
	line_strong: 0x47556c,
	text: 0xdee2ea,
	text_body: 0xc7ceda,
	text_muted: 0x9faaba,
	accent: 0x85aff7,
	primary_fill: 0x426fbf,
	control_active_fill: 0x54698e,
	on_primary: 0xffffff,
	blue: 0x85aff7,
	green: 0xa0baa9,
	amber: 0xd4b58a,
	error: 0xdf969c,
	status_info: 0x9eb8df,
	status_success: 0x9bc7ab,
	status_warning: 0xd6b99a,
	status_error: 0xdf969c,
	navigation_bases: [
		0x91afd4, 0x83b8b1, 0xadb994, 0xc3ac7e, 0xc19683, 0xbf9aa9, 0xac9ac8, 0x9fabbc,
	],
	mascot_bases: [
		0x89a6ca, 0xa89bc7, 0x7da1dc, 0xc89c8d, 0xc2ad8e, 0xbdc3cd, 0xc69aac, 0x969fcb, 0x95b3d4,
		0x98a8bd, 0xd2939b, 0x7e94ba,
	],
};
// The desktop light palette preserves the original ink, surface hierarchy and identity slots.
const DECODEX_LIGHT: Palette = Palette {
	canvas: 0xf5f6f9,
	shell_material: tint(0xeef1f6, SHELL_MATERIAL),
	content_material: tint(0xfafbfc, CONTENT_MATERIAL),
	topbar_material: tint(0xf0f2f6, TOPBAR_MATERIAL),
	agent_sidebar_material: tint(0xf0f2f6, AGENT_SIDEBAR_MATERIAL),
	agent_chat_overlay: tint(0xf0f2f6, AGENT_CHAT_OVERLAY),
	sidebar_material: tint(0xeef1f6, SIDEBAR_MATERIAL),
	surface_raised_material: tint(0xf0f2f6, SURFACE_RAISED_MATERIAL),
	composer_material: tint(0xffffff, COMPOSER_MATERIAL),
	message_material: tint(0xf0f2f7, MESSAGE_MATERIAL),
	surface_overlay_material: tint(0xffffff, SURFACE_OVERLAY_MATERIAL),
	surface_hover: 0xe0e6f0,
	selection_fill: tint(0x2e63c2, SELECTED_FILL),
	selection_hover: tint(0x2e63c2, SELECTED_HOVER_FILL),
	selection_border: tint(0x2e63c2, SELECTED_BORDER),
	control_selected_fill: tint(0x252c39, CONTROL_SELECTED_FILL),
	control_selected_hover: tint(0x252c39, CONTROL_SELECTED_HOVER),
	control_selected_border: tint(0x252c39, CONTROL_SELECTED_BORDER),
	line_strong: 0xb7c2d2,
	text: 0x252c39,
	text_body: 0x45536a,
	text_muted: 0x59687e,
	accent: 0x2e63c2,
	primary_fill: 0x4766b8,
	control_active_fill: 0x526ca6,
	on_primary: 0xffffff,
	blue: 0x2e63c2,
	green: 0x42765a,
	amber: 0x996124,
	error: 0xb43a46,
	status_info: 0x2e63a8,
	status_success: 0x2c7050,
	status_warning: 0x94611d,
	status_error: 0xb43a46,
	navigation_bases: [
		0x4a6c92, 0x4f7c78, 0x64764b, 0x8c7243, 0x956b57, 0x946b80, 0x776294, 0x626f85,
	],
	mascot_bases: [
		0x617590, 0x736e8e, 0x59729a, 0x866f6b, 0x83796c, 0x808691, 0x856e7e, 0x687190, 0x687d96,
		0x6a7688, 0x8c6973, 0x5a6a86,
	],
};
// Palette sources; material alpha and semantic role assignment are Decodex-specific.
// https://github.com/folke/tokyonight.nvim/blob/main/extras/lua/tokyonight_night.lua
const TOKYO_NIGHT: Palette = Palette::from_colors(
	[0x1a1b26, 0x16161e, 0x292e42],
	[0xc0caf5, 0xa9b1d6, 0x414868],
	[0xbb9af7, 0x7aa2f7, 0x9ece6a, 0xe0af68, 0xf7768e],
);
// https://github.com/EdenEast/nightfox.nvim/blob/main/lua/nightfox/palette/nightfox.lua
const NIGHTFOX: Palette = Palette::from_colors(
	[0x192330, 0x131a24, 0x212e3f],
	[0xcdcecf, 0xaeafb0, 0x39506d],
	[0x719cd6, 0x719cd6, 0x81b29a, 0xdbc074, 0xc94f6d],
);
// https://github.com/catppuccin/palette/blob/main/palette.json (Mocha)
const CATPPUCCIN_MOCHA: Palette = Palette::from_colors(
	[0x1e1e2e, 0x181825, 0x313244],
	[0xcdd6f4, 0xa6adc8, 0x45475a],
	[0xcba6f7, 0x89b4fa, 0xa6e3a1, 0xf9e2af, 0xf38ba8],
);
// https://github.com/dracula/dracula-theme#color-palette-oss
const DRACULA: Palette = Palette::from_colors(
	[0x282a36, 0x21222c, 0x44475a],
	[0xf8f8f2, 0xbfc1d6, 0x6272a4],
	[0xbd93f9, 0x8be9fd, 0x50fa7b, 0xf1fa8c, 0xff5555],
);
// https://github.com/nordtheme/nord/blob/develop/src/nord.css
const NORD: Palette = Palette::from_colors(
	[0x2e3440, 0x2e3440, 0x3b4252],
	[0xeceff4, 0xd8dee9, 0x4c566a],
	[0x88c0d0, 0x81a1c1, 0xa3be8c, 0xebcb8b, 0xbf616a],
);
// https://github.com/morhetz/gruvbox/blob/master/colors/gruvbox.vim
const GRUVBOX_DARK: Palette = Palette::from_colors(
	[0x282828, 0x1d2021, 0x3c3836],
	[0xebdbb2, 0xbdae93, 0x665c54],
	[0xfe8019, 0x83a598, 0xb8bb26, 0xfabd2f, 0xfb4934],
);
// https://github.com/atom/one-dark-syntax/blob/master/styles/colors.less
const ONE_DARK: Palette = Palette::from_colors(
	[0x282c34, 0x21252b, 0x323842],
	[0xabb2bf, 0x9da5b4, 0x4b5263],
	[0xc678dd, 0x61afef, 0x98c379, 0xe5c07b, 0xe06c75],
);
// https://github.com/ayu-theme/ayu-colors/blob/master/themes/dark.yaml
// Secondary text is brighter than the original comments to preserve UI readability.
const AYU_DARK: Palette = Palette::from_colors(
	[0x10141c, 0x0d1017, 0x141821],
	[0xbfbdb6, 0x8a9199, 0x3e4b59],
	[0xe6b450, 0x59c2ff, 0xaad94c, 0xffb454, 0xf07178],
);
// https://github.com/rose-pine/neovim/blob/main/lua/rose-pine/palette.lua
const ROSE_PINE: Palette = Palette::from_colors(
	[0x191724, 0x1f1d2e, 0x26233a],
	[0xe0def4, 0x908caa, 0x524f67],
	[0xc4a7e7, 0x9ccfd8, 0x95b1ac, 0xf6c177, 0xeb6f92],
);
// https://github.com/rebelot/kanagawa.nvim/blob/master/lua/kanagawa/colors.lua (Wave)
const KANAGAWA: Palette = Palette::from_colors(
	[0x1f1f28, 0x16161d, 0x2a2a37],
	[0xdcd7ba, 0xc8c093, 0x54546d],
	[0x957fb8, 0x7e9cd8, 0x98bb6c, 0xe6c384, 0xff5d62],
);
// https://github.com/sainnhe/everforest/blob/master/autoload/everforest.vim (Dark hard)
const EVERFOREST: Palette = Palette::from_colors(
	[0x272e33, 0x1e2326, 0x2e383c],
	[0xd3c6aa, 0x9da9a0, 0x4f5b58],
	[0xa7c080, 0x7fbbb3, 0xa7c080, 0xdbbc7f, 0xe67e80],
);
// https://ethanschoonover.com/solarized/#the-values
// Use the lighter neutral tones for UI text on raised surfaces.
const SOLARIZED_DARK: Palette = Palette::from_colors(
	[0x002b36, 0x002b36, 0x073642],
	[0xeee8d5, 0x93a1a1, 0x586e75],
	[0x2aa198, 0x268bd2, 0x859900, 0xb58900, 0xdc322f],
);
// https://github.com/primer/github-vscode-theme/blob/main/src/colors.js
// Uses @primer/primitives 7.10.0, with the theme's foreground override.
const GITHUB_DARK: Palette = Palette::from_colors(
	[0x0d1117, 0x010409, 0x161b22],
	[0xe6edf3, 0x8b949e, 0x30363d],
	[0xa371f7, 0x58a6ff, 0x3fb950, 0xd29922, 0xf85149],
);

// Light variants use the published canvas and color ramps. UI ink is adjusted where an
// editor's syntax or comment color would be too faint on the corresponding raised surface.
// https://github.com/folke/tokyonight.nvim/blob/main/extras/lua/tokyonight_day.lua
const TOKYO_DAY: Palette = Palette::light(
	[0xe1e2e7, 0xd0d5e3, 0xd0d5e3],
	[shade(0x3760bf, 30), shade(0x6172b0, 25), 0xb4b5b9],
	[
		shade(0x7847bd, 10),
		shade(0x2e7de9, 30),
		shade(0x587539, 20),
		shade(0x8c6c3e, 20),
		shade(0xc64343, 20),
	],
);
// https://github.com/EdenEast/nightfox.nvim/blob/main/lua/nightfox/palette/dayfox.lua
const DAYFOX: Palette = Palette::light(
	[0xf6f2ee, 0xe4dcd4, 0xdbd1dd],
	[0x3d2b5a, 0x643f61, 0xaab0ad],
	[0x2848a9, 0x2848a9, shade(0x396847, 5), shade(0xac5402, 20), 0xa5222f],
);
// https://catppuccin.com/palette/#flavor-latte
const CATPPUCCIN_LATTE: Palette = Palette::light(
	[0xeff1f5, 0xdce0e8, 0xe6e9ef],
	[0x4c4f69, 0x5c5f77, 0xbcc0cc],
	[
		shade(0x8839ef, 10),
		shade(0x1e66f5, 15),
		shade(0x40a02b, 35),
		shade(0xdf8e1d, 40),
		shade(0xd20f39, 10),
	],
);
// https://draculatheme.com/spec#color-palette (Alucard Classic)
const ALUCARD: Palette = Palette::light(
	[0xfffbeb, 0xece9df, 0xefeddc],
	[0x1f1f1f, 0x6c664b, 0xcfcfde],
	[0x644ac9, 0x036a96, 0x14710a, shade(0x846e15, 10), shade(0xcb3a2a, 10)],
);
// Nord specifies Snow Storm surfaces and Polar Night text for bright designs.
// https://www.nordtheme.com/docs/colors-and-palettes/
// https://github.com/nord-light/.github/blob/main/profile/README.md (independent editor ports)
const NORD_LIGHT: Palette = Palette::light(
	[0xeceff4, 0xe5e9f0, 0xd8dee9],
	[0x2e3440, 0x4c566a, 0xc8d0da],
	[shade(0x0c7482, 10), 0x3b5e85, shade(0x577140, 10), shade(0x7f6614, 10), shade(0xae4750, 10)],
);
// https://github.com/morhetz/gruvbox/blob/master/colors/gruvbox.vim (light, medium)
const GRUVBOX_LIGHT: Palette = Palette::light(
	[0xfbf1c7, 0xebdbb2, 0xf2e5bc],
	[0x3c3836, 0x665c54, 0xbdae93],
	[shade(0xaf3a03, 5), 0x076678, shade(0x79740e, 20), shade(0xb57614, 30), 0x9d0006],
);
// https://github.com/atom/one-light-syntax/blob/master/styles/colors.less
const ONE_LIGHT: Palette = Palette::light(
	[0xfafafa, 0xe5e5e6, 0xf0f0f1],
	[0x383a42, shade(0x696c77, 10), 0xa0a1a7],
	[0xa626a4, shade(0x4078f2, 20), shade(0x50a14f, 30), shade(0x986801, 15), shade(0xe45649, 25)],
);
// https://github.com/ayu-theme/ayu-colors/blob/master/themes/light.yaml
const AYU_LIGHT: Palette = Palette::light(
	[0xfcfcfc, 0xebeef0, 0xf8f9fa],
	[shade(mix(0xfcfcfc, 0x020a12, 64), 5), 0x626a73, 0xadaeb1],
	[
		shade(0xf29718, 40),
		shade(0x22a4e6, 35),
		shade(0x86b300, 40),
		shade(0xeba400, 45),
		shade(0xe65050, 20),
	],
);
// https://github.com/rose-pine/neovim/blob/main/lua/rose-pine/palette.lua (Dawn)
const ROSE_PINE_DAWN: Palette = Palette::light(
	[0xfaf4ed, 0xfffaf3, 0xf2e9e1],
	[0x464261, shade(0x797593, 15), 0xcecacd],
	[shade(0x907aa9, 25), 0x286983, shade(0x6d8f89, 25), shade(0xea9d34, 40), shade(0xb4637a, 20)],
);
// https://github.com/rebelot/kanagawa.nvim/blob/master/lua/kanagawa/colors.lua (Lotus)
const KANAGAWA_LOTUS: Palette = Palette::light(
	[0xf2ecbc, 0xe5ddb0, 0xe7dba0],
	[shade(0x545464, 10), shade(0x716e61, 15), 0xd5cea3],
	[0x624c83, shade(0x4d699b, 10), shade(0x6f894e, 30), shade(0x836f4a, 20), shade(0xc84053, 20)],
);
// https://github.com/sainnhe/everforest/blob/master/autoload/everforest.vim (Light hard)
const EVERFOREST_LIGHT: Palette = Palette::light(
	[0xfffbef, 0xf2efdf, 0xf8f5e4],
	[shade(0x5c6a72, 15), shade(0x829181, 25), 0xa6b0a0],
	[
		shade(0x8da101, 30),
		shade(0x3a94c5, 25),
		shade(0x8da101, 30),
		shade(0xdfa000, 40),
		shade(0xf85552, 25),
	],
);
// https://ethanschoonover.com/solarized/#the-values (base3 and base2)
const SOLARIZED_LIGHT: Palette = Palette::light(
	[0xfdf6e3, 0xeee8d5, 0xeee8d5],
	[shade(0x586e75, 20), shade(0x657b83, 15), 0x93a1a1],
	[
		shade(0x2aa198, 30),
		shade(0x268bd2, 25),
		shade(0x859900, 30),
		shade(0xb58900, 30),
		shade(0xdc322f, 15),
	],
);
// https://github.com/primer/github-vscode-theme/blob/main/src/colors.js (Primer Light)
const GITHUB_LIGHT: Palette = Palette::light(
	[0xffffff, 0xf6f8fa, 0xf6f8fa],
	[0x24292f, 0x57606a, 0xd0d7de],
	[0x8250df, 0x0969da, 0x1a7f37, shade(0x9a6700, 5), 0xcf222e],
);

#[cfg(not(test))]
static SELECTED: AtomicU8 = AtomicU8::new(u8::MAX);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) enum ColorTheme {
	#[default]
	Decodex,
	TokyoNight,
	Nightfox,
	Catppuccin,
	Dracula,
	Nord,
	Gruvbox,
	AtomOne,
	Ayu,
	RosePine,
	Kanagawa,
	Everforest,
	Solarized,
	Github,
}
impl ColorTheme {
	// The product theme stays first; presets follow their display names alphabetically.
	pub(crate) const ALL: [Self; 14] = [
		Self::Decodex,
		Self::AtomOne,
		Self::Ayu,
		Self::Catppuccin,
		Self::Dracula,
		Self::Everforest,
		Self::Github,
		Self::Gruvbox,
		Self::Kanagawa,
		Self::Nightfox,
		Self::Nord,
		Self::RosePine,
		Self::Solarized,
		Self::TokyoNight,
	];

	pub(crate) fn id(self) -> &'static str {
		match self {
			Self::Decodex => "decodex",
			Self::TokyoNight => "tokyo-night",
			Self::Nightfox => "nightfox",
			Self::Catppuccin => "catppuccin-mocha",
			Self::Dracula => "dracula",
			Self::Nord => "nord",
			Self::Gruvbox => "gruvbox-dark",
			Self::AtomOne => "one-dark",
			Self::Ayu => "ayu-dark",
			Self::RosePine => "rose-pine",
			Self::Kanagawa => "kanagawa",
			Self::Everforest => "everforest",
			Self::Solarized => "solarized-dark",
			Self::Github => "github-dark",
		}
	}

	pub(crate) fn name(self) -> &'static str {
		match self {
			Self::Decodex => "Decodex",
			Self::TokyoNight => "Tokyo Night",
			Self::Nightfox => "Nightfox",
			Self::Catppuccin => "Catppuccin",
			Self::Dracula => "Dracula",
			Self::Nord => "Nord",
			Self::Gruvbox => "Gruvbox",
			Self::AtomOne => "Atom One",
			Self::Ayu => "Ayu",
			Self::RosePine => "Rosé Pine",
			Self::Kanagawa => "Kanagawa",
			Self::Everforest => "Everforest",
			Self::Solarized => "Solarized",
			Self::Github => "GitHub",
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

		let theme = ui_preferences::string(PREFERENCE, None)
			.and_then(|id| Self::from_id(&id))
			.unwrap_or_default();

		theme.cache();

		theme
	}

	fn cache(self) {
		#[cfg(not(test))]
		SELECTED.store(
			Self::ALL.iter().position(|theme| *theme == self).expect("built-in theme in catalog")
				as u8,
			Ordering::Relaxed,
		);
		#[cfg(test)]
		SELECTED.set(Some(self));
	}

	pub(crate) fn select(self, cx: &mut App) {
		ui_preferences::string(PREFERENCE, Some(self.id()));

		self.apply(cx);
	}

	/// Apply a palette to every window. Capture fixtures can use this without saving it.
	pub(crate) fn apply(self, cx: &mut App) {
		self.cache();
		cx.refresh_windows();
	}

	pub(crate) fn palette(self, appearance: Theme) -> &'static Palette {
		let (dark, light) = match self {
			Self::Decodex => (&DECODEX_DARK, &DECODEX_LIGHT),
			Self::TokyoNight => (&TOKYO_NIGHT, &TOKYO_DAY),
			Self::Nightfox => (&NIGHTFOX, &DAYFOX),
			Self::Catppuccin => (&CATPPUCCIN_MOCHA, &CATPPUCCIN_LATTE),
			Self::Dracula => (&DRACULA, &ALUCARD),
			Self::Nord => (&NORD, &NORD_LIGHT),
			Self::Gruvbox => (&GRUVBOX_DARK, &GRUVBOX_LIGHT),
			Self::AtomOne => (&ONE_DARK, &ONE_LIGHT),
			Self::Ayu => (&AYU_DARK, &AYU_LIGHT),
			Self::RosePine => (&ROSE_PINE, &ROSE_PINE_DAWN),
			Self::Kanagawa => (&KANAGAWA, &KANAGAWA_LOTUS),
			Self::Everforest => (&EVERFOREST, &EVERFOREST_LIGHT),
			Self::Solarized => (&SOLARIZED_DARK, &SOLARIZED_LIGHT),
			Self::Github => (&GITHUB_DARK, &GITHUB_LIGHT),
		};
		match appearance {
			Theme::Dark => dark,
			Theme::Light => light,
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
	pub(crate) message_material: u32,
	pub(crate) surface_overlay_material: u32,
	pub(crate) surface_hover: u32,
	pub(crate) selection_fill: u32,
	pub(crate) selection_hover: u32,
	pub(crate) selection_border: u32,
	pub(crate) control_selected_fill: u32,
	pub(crate) control_selected_hover: u32,
	pub(crate) control_selected_border: u32,
	pub(crate) line_strong: u32,
	pub(crate) text: u32,
	pub(crate) text_body: u32,
	pub(crate) text_muted: u32,
	pub(crate) accent: u32,
	pub(crate) primary_fill: u32,
	pub(crate) control_active_fill: u32,
	pub(crate) on_primary: u32,
	pub(crate) blue: u32,
	pub(crate) green: u32,
	pub(crate) amber: u32,
	pub(crate) error: u32,
	pub(crate) status_info: u32,
	pub(crate) status_success: u32,
	pub(crate) status_warning: u32,
	pub(crate) status_error: u32,
	/// Stable project-color slots; each theme supplies its own related hues.
	pub(crate) navigation_bases: [u32; 8],
	// Catalog order: mint, lilac, sky, peach, butter, cream, rose,
	// periwinkle, ice, sage, coral, midnight. Identity does not depend on theme.
	pub(crate) mascot_bases: [u32; 12],
}
impl Palette {
	pub(super) fn material(&self, token: u32) -> Option<u32> {
		Some(match token {
			SHELL_MATERIAL => self.shell_material,
			CONTENT_MATERIAL => self.content_material,
			TOPBAR_MATERIAL => self.topbar_material,
			AGENT_SIDEBAR_MATERIAL => self.agent_sidebar_material,
			AGENT_CHAT_OVERLAY => self.agent_chat_overlay,
			SIDEBAR_MATERIAL => self.sidebar_material,
			SURFACE_RAISED_MATERIAL => self.surface_raised_material,
			COMPOSER_MATERIAL => self.composer_material,
			MESSAGE_MATERIAL => self.message_material,
			SURFACE_OVERLAY_MATERIAL => self.surface_overlay_material,
			SELECTED_FILL => self.selection_fill,
			SELECTED_HOVER_FILL => self.selection_hover,
			SELECTED_BORDER => self.selection_border,
			CONTROL_SELECTED_FILL => self.control_selected_fill,
			CONTROL_SELECTED_HOVER => self.control_selected_hover,
			CONTROL_SELECTED_BORDER => self.control_selected_border,
			_ => return None,
		})
	}

	/// Map the shared semantic tokens without changing authored content colors.
	pub(super) fn resolve(&self, token: u32) -> u32 {
		match token {
			PRIMARY_FILL => self.primary_fill,
			CONTROL_ACTIVE_FILL => self.control_active_fill,
			ON_PRIMARY => self.on_primary,
			INFO => self.status_info,
			SUCCESS => self.status_success,
			WARNING => self.status_warning,
			ERROR => self.status_error,
			0x0b0a0f => self.canvas,
			0x101014 => self.shell_material >> 8,
			0x141419 | 0x141418 | 0x111113 => self.content_material >> 8,
			0x15151b => self.topbar_material >> 8,
			0x17171c | 0x17171a => self.agent_sidebar_material >> 8,
			0x100e15 => self.sidebar_material >> 8,
			0x17151e => self.surface_raised_material >> 8,
			0x1d1a24 | 0x24242a => self.surface_overlay_material >> 8,
			0x222228 | 0x27272b => self.composer_material >> 8,
			0x222c3a => self.message_material >> 8,
			0x242427 | 0x29292d | 0x29292e | 0x252830 | 0x303030 =>
				self.surface_raised_material >> 8,
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
			0xed8585 | 0xdc2626 => self.error,
			_ => token,
		}
	}

	// A light editor accent may be suitable on white but unreadable on panels.
	// Each entry supplies the light family's own surfaces and inks. Adjust the
	// semantic colors toward dark ink for readable controls, links and statuses.
	const fn light(backgrounds: [u32; 3], foregrounds: [u32; 3], accents: [u32; 5]) -> Self {
		let mut palette = Self::from_colors(backgrounds, foregrounds, accents);
		palette.text_body = mix(palette.text, palette.text_muted, 25);
		palette.on_primary = 0xffffff;
		palette.control_active_fill = mix(palette.accent, palette.text, 18);
		palette.status_info = palette.blue;
		palette.status_success = palette.green;
		palette.status_warning = palette.amber;
		palette.status_error = palette.error;
		palette
	}

	// Backgrounds: canvas, sidebar, raised. Foregrounds: text, muted, border.
	// Accents: primary, link, success, warning, error. Keep native glass translucent.
	const fn from_colors(backgrounds: [u32; 3], foregrounds: [u32; 3], accents: [u32; 5]) -> Self {
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
			message_material: (raised << 8) | 0x94,
			surface_overlay_material: (raised << 8) | 0xd8,
			surface_hover: raised,
			selection_fill: (accent << 8) | 0x1c,
			selection_hover: (accent << 8) | 0x24,
			selection_border: (accent << 8) | 0x88,
			control_selected_fill: (text << 8) | 0x18,
			control_selected_hover: (accent << 8) | 0x24,
			control_selected_border: (text << 8) | 0x0c,
			line_strong: border,
			text,
			text_body: text,
			text_muted: muted,
			accent,
			primary_fill: accent,
			control_active_fill: accent,
			on_primary: canvas,
			blue,
			green,
			amber,
			error,
			// Small status labels need more light than large chart marks.
			status_info: mix(blue, text, 25),
			status_success: mix(green, text, 12),
			status_warning: mix(amber, text, 15),
			status_error: mix(error, text, 60),
			navigation_bases: [
				mix(blue, text, 20),
				mix(green, blue, 35),
				mix(green, amber, 30),
				mix(amber, text, 20),
				mix(amber, error, 55),
				mix(error, accent, 50),
				mix(accent, blue, 20),
				mix(text, blue, 45),
			],
			mascot_bases: [
				mix(blue, green, 35),
				mix(accent, blue, 25),
				blue,
				mix(error, amber, 25),
				amber,
				mix(text, amber, 12),
				error,
				mix(blue, accent, 45),
				mix(blue, text, 25),
				green,
				mix(error, amber, 12),
				mix(blue, canvas, 18),
			],
		}
	}
}

const fn tint(rgb: u32, role: u32) -> u32 {
	(rgb << 8) | (role & 0xff)
}

// Preserve the source hue when an editor color needs more contrast for a UI role.
const fn shade(source: u32, percent: u32) -> u32 {
	mix(source, 0x000000, percent)
}

/// Mix authored palette channels once, before character geometry is cached.
pub(super) const fn mix(a: u32, b: u32, percent: u32) -> u32 {
	const fn channel(a: u32, b: u32, percent: u32, shift: u32) -> u32 {
		(((a >> shift) & 255) * (100 - percent) + ((b >> shift) & 255) * percent) / 100
	}

	(channel(a, b, percent, 16) << 16)
		| (channel(a, b, percent, 8) << 8)
		| channel(a, b, percent, 0)
}

#[cfg(test)]
mod tests {
	use crate::ui_theme::color_themes::ColorTheme;

	#[test]
	fn preference_ids_round_trip_and_unknown_ids_are_rejected() {
		for theme in ColorTheme::ALL {
			assert_eq!(ColorTheme::from_id(theme.id()), Some(theme));
			assert_eq!(ColorTheme::ALL.iter().filter(|other| other.id() == theme.id()).count(), 1);
		}

		assert_eq!(ColorTheme::from_id("removed-theme"), None);
	}

	#[test]
	fn catalog_pins_decodex_then_sorts_presets_by_name() {
		assert_eq!(ColorTheme::ALL[0], ColorTheme::Decodex);
		assert!(
			ColorTheme::ALL[1..]
				.windows(2)
				.all(|pair| { pair[0].name().to_lowercase() < pair[1].name().to_lowercase() })
		);
	}

	#[test]
	fn saved_dark_variant_ids_select_the_family_in_both_appearances() {
		for (id, name) in [
			("ayu-dark", "Ayu"),
			("catppuccin-mocha", "Catppuccin"),
			("dracula", "Dracula"),
			("github-dark", "GitHub"),
			("gruvbox-dark", "Gruvbox"),
			("one-dark", "Atom One"),
			("solarized-dark", "Solarized"),
		] {
			assert_eq!(ColorTheme::from_id(id).unwrap().name(), name);
		}
		for theme in ColorTheme::ALL {
			assert_ne!(
				theme.palette(super::Theme::Dark).canvas,
				theme.palette(super::Theme::Light).canvas,
				"{theme:?}"
			);
		}
		assert_eq!(ColorTheme::Dracula.palette(super::Theme::Light).canvas, 0xfffbeb);
		assert_eq!(ColorTheme::Nord.palette(super::Theme::Light).canvas, 0xeceff4);
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
			for appearance in [super::Theme::Light, super::Theme::Dark] {
				let palette = theme.palette(appearance);
				for background in [
					palette.canvas,
					palette.composer_material >> 8,
					palette.message_material >> 8,
					palette.surface_overlay_material >> 8,
					palette.surface_hover,
				] {
					for text in [palette.text, palette.text_body, palette.text_muted] {
						let ink = luminance(text) + 0.05;
						let surface = luminance(background) + 0.05;
						let contrast = match appearance {
							super::Theme::Light => surface / ink,
							super::Theme::Dark => ink / surface,
						};
						assert!(
							contrast >= 4.5,
							"{} {appearance:?}: {text:06x} on {background:06x}: {contrast}",
							theme.name()
						);
					}
				}
			}
		}
	}
}
