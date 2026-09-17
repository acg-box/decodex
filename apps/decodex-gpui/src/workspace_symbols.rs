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
}

static IMAGES: LazyLock<[Arc<Image>; 15]> = LazyLock::new(|| {
	let sources: [&[u8]; 15] = [
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
