//! Native macOS SF Symbols, rendered at 3x and embedded for bundle-independent use.
use std::{
	f32::consts::FRAC_PI_2,
	sync::{Arc, LazyLock},
};

use gpui::{
	self, AnyElement, App, ElementId, Image, ImageFormat, PathBuilder, RenderOnce, Window,
	prelude::{IntoElement, Styled as _},
};

use crate::{ui_motion, ui_theme::TEXT_MUTED};

static IMAGES: LazyLock<[Arc<Image>; 36]> = LazyLock::new(|| {
	let sources: [&[u8]; 36] = [
		include_bytes!("../../../assets/workspace-symbols/sidebar.png"),
		include_bytes!("../../../assets/workspace-symbols/graph.png"),
		include_bytes!("../../../assets/workspace-symbols/expand.png"),
		include_bytes!("../../../assets/workspace-symbols/settings.png"),
		include_bytes!("../../../assets/workspace-symbols/close.png"),
		include_bytes!("../../../assets/workspace-symbols/plus.png"),
		include_bytes!("../../../assets/workspace-symbols/minus.png"),
		include_bytes!("../../../assets/workspace-symbols/back.png"),
		include_bytes!("../../../assets/workspace-symbols/forward.png"),
		include_bytes!("../../../assets/workspace-symbols/agents.png"),
		include_bytes!("../../../assets/workspace-symbols/fast.png"),
		include_bytes!("../../../assets/workspace-symbols/chevron-down.png"),
		include_bytes!("../../../assets/workspace-symbols/microphone.png"),
		include_bytes!("../../../assets/workspace-symbols/bell.png"),
		include_bytes!("../../../assets/workspace-symbols/bell-attention.png"),
		include_bytes!("../../../assets/workspace-symbols/bell-info.png"),
		include_bytes!("../../../assets/workspace-symbols/bell-error.png"),
		include_bytes!("../../../assets/workspace-symbols/arrow-down.png"),
		include_bytes!("../../../assets/workspace-symbols/account-route.png"),
		include_bytes!("../../../assets/workspace-symbols/account-logout.png"),
		include_bytes!("../../../assets/workspace-symbols/confirm.png"),
		include_bytes!("../../../assets/workspace-symbols/account-route-active.png"),
		include_bytes!("../../../assets/workspace-symbols/power-on.png"),
		include_bytes!("../../../assets/workspace-symbols/power-off.png"),
		include_bytes!("../../../assets/workspace-symbols/eye.png"),
		include_bytes!("../../../assets/workspace-symbols/eye-slash.png"),
		include_bytes!("../../../assets/workspace-symbols/lock.png"),
		include_bytes!("../../../assets/workspace-symbols/account-sign-in.png"),
		include_bytes!("../../../assets/workspace-symbols/account-warning.png"),
		include_bytes!("../../../assets/workspace-symbols/reset-cards.png"),
		include_bytes!("../../../assets/workspace-symbols/account-reorder.png"),
		include_bytes!("../../../assets/workspace-symbols/account-warning-amber.png"),
		include_bytes!("../../../assets/workspace-symbols/all-work.png"),
		include_bytes!("../../../assets/workspace-symbols/arrange.png"),
		include_bytes!("../../../assets/workspace-symbols/fit.png"),
		include_bytes!("../../../assets/workspace-symbols/waveform.png"),
	];
	sources.map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec())))
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
}

#[derive(gpui::IntoElement)]
pub(super) struct DisclosureChevron {
	id: &'static str,
	expanded: bool,
}
impl RenderOnce for DisclosureChevron {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let progress = ui_motion::value(self.id, if self.expanded { 1. } else { 0. }, window, cx);
		if progress == 0. || progress == 1. {
			return gpui::svg()
				.data(br#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 12 12"><path d="M2 4L6 8L10 4" fill="none" stroke="white" stroke-width="1.2"/></svg>"#)
				.text_color(gpui::rgb(TEXT_MUTED))
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
					window.paint_path(path, gpui::rgb(TEXT_MUTED));
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
	gpui::img(IMAGES[symbol as usize].clone()).size(gpui::px(size)).flex_none().into_any_element()
}

pub(super) fn disclosure_chevron(id: &'static str, expanded: bool) -> DisclosureChevron {
	DisclosureChevron { id, expanded }
}

pub(super) fn process_chevron(id: impl Into<ElementId>, expanded: bool) -> ProcessChevron {
	ProcessChevron { id: id.into(), expanded }
}
