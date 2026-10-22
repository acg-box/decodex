//! Native macOS SF Symbols, rendered at 3x and embedded for bundle-independent use.
use std::{
	f32::consts::FRAC_PI_2,
	sync::{Arc, LazyLock},
};

use gpui::{
	self, AnyElement, App, ElementId, Image, ImageFormat, PathBuilder, RenderOnce, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
	},
};

use crate::{ui_motion, ui_theme::TEXT_MUTED};

// Preserve the existing dark artwork and load its paired light palette on demand.
macro_rules! symbol_sources {
	($($name:literal),* $(,)?) => {[
		[$(include_bytes!(concat!("../../../assets/workspace-symbols/", $name, ".png")) as &[u8]),*],
		[$(include_bytes!(concat!("../../../assets/workspace-symbols/light/", $name, ".png")) as &[u8]),*],
	]};
}
static IMAGES: LazyLock<[[Arc<Image>; 39]; 2]> = LazyLock::new(|| {
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
		"account-warning",
		"reset-cards",
		"account-reorder",
		"account-warning-amber",
		"all-work",
		"arrange",
		"fit",
		"waveform",
		"restore",
		"document",
		"link",
	];
	sources.map(|variant| {
		variant.map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec())))
	})
});

#[derive(Clone, Copy)]
pub(super) enum Symbol {
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
	AccountWarning,
	ResetCards,
	AccountReorder,
	AccountWarningAmber,
	AllWork,
	Arrange,
	Fit,
	Waveform,
	Restore,
	Document,
	Link,
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
				.text_color(crate::ui_theme::rgb(TEXT_MUTED))
				.with_transformation(gpui::Transformation::rotate(gpui::radians(progress * std::f32::consts::PI)))
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
					window.paint_path(path, crate::ui_theme::rgb(TEXT_MUTED));
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
			.with_transformation(gpui::Transformation::rotate(gpui::radians(progress * FRAC_PI_2)))
			.size(gpui::px(12.))
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

pub(super) fn icon_sized(symbol: Symbol, size: f32) -> AnyElement {
	gpui::img(
		IMAGES[usize::from(
			crate::ui_theme::appearance::Theme::current()
				== crate::ui_theme::appearance::Theme::Light,
		)][symbol as usize]
			.clone(),
	)
	.size(gpui::px(size))
	.flex_none()
	.into_any_element()
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
