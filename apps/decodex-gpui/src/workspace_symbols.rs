//! Native macOS SF Symbols, rendered at 3x and embedded for bundle-independent use.
use std::{
	f32::consts::{FRAC_PI_2, PI},
	sync::LazyLock,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui::{
	self, AnyElement, App, ElementId, PathBuilder, RenderOnce, Transformation, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
	},
};

use crate::{
	ui_motion,
	ui_theme::{self, ERROR, INFO, SUCCESS, TEXT_MUTED, WARNING},
};

// GPUI uses an SVG's alpha as a tintable mask. Keep the native symbol artwork
// and cache one template per shape, independent of appearance and status color.
macro_rules! symbol_sources {
	($($name:literal),* $(,)?) => {[
		$(include_bytes!(concat!("../../../assets/workspace-symbols/", $name, ".png")) as &[u8]),*
	]};
}

static TEMPLATES: LazyLock<[String; 53]> = LazyLock::new(|| {
	let sources = symbol_sources![
		"sidebar",
		"graph",
		"expand",
		"settings",
		"close",
		"plus",
		"minus",
		"back",
		"forward",
		"agents",
		"fast",
		"chevron-down",
		"microphone",
		"bell",
		"bell-attention",
		"bell-info",
		"bell-error",
		"arrow-down",
		"account-route",
		"account-logout",
		"confirm",
		"account-route-active",
		"power-on",
		"power-off",
		"eye",
		"eye-slash",
		"lock",
		"account-sign-in",
		"reset-cards",
		"account-reorder",
		"all-work",
		"arrange",
		"fit",
		"waveform",
		"restore",
		"document",
		"link",
		"activity-goal",
		"activity-search",
		"activity-edit",
		"activity-command",
		"activity-handoff",
		"team-members",
		"team-parent",
		"shield",
		"shield-check",
		"shield-slash",
		"shield-lock",
		"archive",
		"unarchive",
		"trash",
		"ultrafast",
		"fork",
	];
	sources.map(|bytes| {
		format!(
			r#"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 48 48"><image width="48" height="48" href="data:image/png;base64,{}"/></svg>"#,
			STANDARD.encode(bytes),
		)
	})
});

#[derive(Clone, Copy)]
pub(crate) enum Symbol {
	Sidebar,
	Graph,
	Expand,
	Settings,
	Close,
	Plus,
	Minus,
	Back,
	Forward,
	Agents,
	Fast,
	ChevronDown,
	Microphone,
	Bell,
	BellAttention,
	BellInfo,
	BellError,
	ArrowDown,
	AccountRoute,
	AccountLogout,
	Confirm,
	AccountRouteActive,
	PowerOn,
	PowerOff,
	Eye,
	EyeSlash,
	Lock,
	AccountSignIn,
	ResetCards,
	AccountReorder,
	AllWork,
	Arrange,
	Fit,
	Waveform,
	Restore,
	Document,
	Link,
	ActivityGoal,
	ActivitySearch,
	ActivityEdit,
	ActivityCommand,
	ActivityHandoff,
	TeamMembers,
	TeamParent,
	Shield,
	ShieldCheck,
	ShieldSlash,
	ShieldLock,
	Archive,
	Unarchive,
	Trash,
	Ultrafast,
	Fork,
}

/// Both panel controls use their layout state and the same interruptible transition.
#[derive(gpui::IntoElement)]
pub(super) struct PanelExpansion {
	id: &'static str,
	expanded: bool,
}
impl RenderOnce for PanelExpansion {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let progress = ui_motion::value(self.id, if self.expanded { 1. } else { 0. }, window, cx);

		gpui::div()
			.relative()
			.size(gpui::px(16.))
			.flex_none()
			.when(progress < 1., |slot| {
				slot.child(
					gpui::div()
						.debug_selector(move || format!("{}-expand-icon", self.id))
						.absolute()
						.inset_0()
						.opacity(1. - progress)
						.child(icon(Symbol::Expand)),
				)
			})
			.when(progress > 0., |slot| {
				slot.child(
					gpui::div()
						.debug_selector(move || format!("{}-restore-icon", self.id))
						.absolute()
						.inset_0()
						.opacity(progress)
						.child(icon(Symbol::Restore)),
				)
			})
	}
}

#[derive(gpui::IntoElement)]
pub(super) struct DisclosureChevron {
	id: ElementId,
	expanded: bool,
}
impl RenderOnce for DisclosureChevron {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let progress = ui_motion::value(self.id, if self.expanded { 1. } else { 0. }, window, cx);

		if progress == 0. || progress == 1. {
			return gpui::svg()
				.data(br#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 12 12"><path d="M2 4L6 8L10 4" fill="none" stroke="white" stroke-width="1.2"/></svg>"#)
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.with_transformation(Transformation::rotate(gpui::radians(progress * PI)))
				.size(gpui::px(12.))
				.flex_none()
				.into_any_element();
		}

		// Only the changing outline needs a live path. Its settled shapes use
		// the atlas without changing the existing fold motion or stroke weight.
		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, _| {
				let direction = 1. - 2. * progress;
				let mut path = PathBuilder::stroke(gpui::px(1.2));

				path.move_to(
					bounds.origin + gpui::point(gpui::px(2.), gpui::px(6. - 2. * direction)),
				);
				path.line_to(
					bounds.origin + gpui::point(gpui::px(6.), gpui::px(6. + 2. * direction)),
				);
				path.line_to(
					bounds.origin + gpui::point(gpui::px(10.), gpui::px(6. - 2. * direction)),
				);

				if let Ok(path) = path.build() {
					window.paint_path(path, ui_theme::rgb(TEXT_MUTED));
				}
			},
		)
		.size(gpui::px(12.))
		.flex_none()
		.into_any_element()
	}
}

/// A single centered chevron rotates instead of exchanging font glyphs.
#[derive(gpui::IntoElement)]
pub(super) struct ProcessChevron {
	id: ElementId,
	expanded: bool,
}
impl RenderOnce for ProcessChevron {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let progress = ui_motion::value(self.id, if self.expanded { 1. } else { 0. }, window, cx);

		// Rasterize this fixed outline once in GPUI's atlas. A live path for every
		// tool row splits Metal rendering into full-window intermediate passes.
		gpui::svg()
			.data(br#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 12 12"><path d="M4 2.5L7.5 6L4 9.5" fill="none" stroke="white" stroke-width="1.4"/></svg>"#)
			.text_color(window.text_style().color)
			.with_transformation(Transformation::rotate(gpui::radians(progress * FRAC_PI_2)))
			.size(gpui::px(12.))
			.flex_none()
	}
}

#[derive(gpui::IntoElement)]
struct SymbolIcon {
	symbol: Symbol,
	size: f32,
}
impl RenderOnce for SymbolIcon {
	fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
		let color = match self.symbol {
			Symbol::BellAttention | Symbol::ShieldSlash => ui_theme::rgb(WARNING).into(),
			Symbol::BellError => ui_theme::rgb(ERROR).into(),
			Symbol::BellInfo | Symbol::ShieldCheck => ui_theme::rgb(INFO).into(),
			Symbol::AccountRouteActive | Symbol::PowerOn => ui_theme::rgb(SUCCESS).into(),
			_ => window.text_style().color,
		};

		gpui::svg()
			.data(TEMPLATES[self.symbol as usize].as_bytes())
			.text_color(color)
			.size(gpui::px(self.size))
			.flex_none()
	}
}

pub(super) fn icon(symbol: Symbol) -> AnyElement {
	let size = match symbol {
		Symbol::ChevronDown => 12.0,
		Symbol::Sidebar | Symbol::Graph | Symbol::Agents => 20.0,
		_ => 16.0,
	};

	icon_sized(symbol, size)
}

pub(super) fn disclosure_chevron(id: impl Into<ElementId>, expanded: bool) -> DisclosureChevron {
	DisclosureChevron { id: id.into(), expanded }
}

pub(super) fn process_chevron(id: impl Into<ElementId>, expanded: bool) -> ProcessChevron {
	ProcessChevron { id: id.into(), expanded }
}

pub(super) fn panel_expansion(id: &'static str, expanded: bool) -> PanelExpansion {
	PanelExpansion { id, expanded }
}

pub(crate) fn icon_sized(symbol: Symbol, size: f32) -> AnyElement {
	SymbolIcon { symbol, size }.into_any_element()
}

/// Reserve one selection slot so a check never shifts its menu label.
pub(crate) fn menu_check(selected: bool) -> gpui::Div {
	gpui::div()
		.size(gpui::px(16.))
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
		.when(selected, |slot| slot.child(icon_sized(Symbol::Confirm, 12.)))
}

#[cfg(test)]
mod tests {
	use crate::shell::workspace_symbols::TEMPLATES;

	#[gpui::test]
	fn native_symbol_templates_preserve_visible_alpha(cx: &mut gpui::TestAppContext) {
		let renderer = cx.update(|cx| cx.svg_renderer());

		for (index, template) in TEMPLATES.iter().enumerate() {
			let image = renderer
				.render_single_frame(template.as_bytes(), 1.)
				.unwrap_or_else(|error| panic!("symbol {index}: {error}"));

			assert_eq!(image.size(0), gpui::size(gpui::DevicePixels(96), gpui::DevicePixels(96)));

			let pixels = image.as_bytes(0).expect("symbol pixels");
			let alpha: Vec<_> = pixels.as_chunks::<4>().0.iter().map(|pixel| pixel[3]).collect();

			assert!(alpha.iter().any(|value| *value != 0), "symbol {index}: empty mask");
			assert!(alpha.contains(&0), "symbol {index}: lost transparent background");
		}
	}
}
