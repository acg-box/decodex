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

static IMAGES: LazyLock<[Arc<Image>; 33]> = LazyLock::new(|| {
	let sources: [&[u8]; 33] = [
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
}

#[derive(gpui::IntoElement)]
pub(super) struct DisclosureChevron {
	id: &'static str,
	expanded: bool,
}
impl RenderOnce for DisclosureChevron {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let progress = ui_motion::value(self.id, if self.expanded { 1. } else { 0. }, window, cx);

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
		let angle = progress * FRAC_PI_2;

		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, _| {
				let mut path = PathBuilder::stroke(gpui::px(1.4));

				for (i, (x, y)) in [(-2., -3.5), (1.5, 0.), (-2., 3.5)].into_iter().enumerate() {
					let point = bounds.center()
						+ gpui::point(
							gpui::px(x * angle.cos() - y * angle.sin()),
							gpui::px(x * angle.sin() + y * angle.cos()),
						);

					if i == 0 {
						path.move_to(point);
					} else {
						path.line_to(point);
					}
				}

				if let Ok(path) = path.build() {
					window.paint_path(path, window.text_style().color);
				}
			},
		)
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
