//! Native macOS SF Symbols, rendered at 3x and embedded for bundle-independent use.
use std::sync::{Arc, LazyLock};

use gpui::{AnyElement, Image, ImageFormat, img, prelude::*, px};

#[derive(Clone, Copy)]
pub(super) enum Symbol {
	Sidebar,
	Graph,
	Timeline,
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
	Voice,
	Microphone,
	Bell,
	BellAttention,
	BellInfo,
	ArrowDown,
	AccountRoute,
	AccountLogin,
	AccountLogout,
	Confirm,
	AccountRouteActive,
	PowerOn,
	PowerOff,
	Eye,
	EyeSlash,
}

static IMAGES: LazyLock<[Arc<Image>; 28]> = LazyLock::new(|| {
	let sources: [&[u8]; 28] = [
		include_bytes!("../../../assets/workspace-symbols/sidebar.png"),
		include_bytes!("../../../assets/workspace-symbols/graph.png"),
		include_bytes!("../../../assets/workspace-symbols/timeline.png"),
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
		include_bytes!("../../../assets/workspace-symbols/voice.png"),
		include_bytes!("../../../assets/workspace-symbols/microphone.png"),
		include_bytes!("../../../assets/workspace-symbols/bell.png"),
		include_bytes!("../../../assets/workspace-symbols/bell-attention.png"),
		include_bytes!("../../../assets/workspace-symbols/bell-info.png"),
		include_bytes!("../../../assets/workspace-symbols/arrow-down.png"),
		include_bytes!("../../../assets/workspace-symbols/account-route.png"),
		include_bytes!("../../../assets/workspace-symbols/account-login.png"),
		include_bytes!("../../../assets/workspace-symbols/account-logout.png"),
		include_bytes!("../../../assets/workspace-symbols/confirm.png"),
		include_bytes!("../../../assets/workspace-symbols/account-route-active.png"),
		include_bytes!("../../../assets/workspace-symbols/power-on.png"),
		include_bytes!("../../../assets/workspace-symbols/power-off.png"),
		include_bytes!("../../../assets/workspace-symbols/eye.png"),
		include_bytes!("../../../assets/workspace-symbols/eye-slash.png"),
	];
	sources.map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec())))
});

pub(super) fn icon(symbol: Symbol) -> AnyElement {
	let size = match symbol {
		Symbol::ChevronDown => 12.0,
		Symbol::Sidebar | Symbol::Graph | Symbol::Timeline | Symbol::Agents => 20.0,
		_ => 16.0,
	};
	img(IMAGES[symbol as usize].clone()).size(px(size)).flex_none().into_any_element()
}

#[derive(gpui::IntoElement)]
pub(super) struct DisclosureChevron {
	id: &'static str,
	expanded: bool,
}

pub(super) fn disclosure_chevron(id: &'static str, expanded: bool) -> DisclosureChevron {
	DisclosureChevron { id, expanded }
}

impl gpui::RenderOnce for DisclosureChevron {
	fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
		let progress =
			crate::ui_motion::value(self.id, if self.expanded { 1. } else { 0. }, window, cx);
		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, _| {
				let direction = 1. - 2. * progress;
				let mut path = gpui::PathBuilder::stroke(px(1.2));
				path.move_to(bounds.origin + gpui::point(px(2.), px(6. - 2. * direction)));
				path.line_to(bounds.origin + gpui::point(px(6.), px(6. + 2. * direction)));
				path.line_to(bounds.origin + gpui::point(px(10.), px(6. - 2. * direction)));
				if let Ok(path) = path.build() {
					window.paint_path(path, gpui::rgb(crate::ui_theme::TEXT_MUTED));
				}
			},
		)
		.size(px(12.))
		.flex_none()
	}
}
