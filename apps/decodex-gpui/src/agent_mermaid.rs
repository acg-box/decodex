//! Ondine owns Mermaid syntax and layout; the desktop owns presentation and resource limits.
#[path = "agent_mermaid/svg.rs"] mod svg;

use crate::ui_theme::appearance::Theme;
use gpui::{RenderImage, SvgRenderer};
use std::{
	sync::Arc,
	time::{SystemTime, UNIX_EPOCH},
};

// Bound decoded images as well as SVG text. A small SVG can describe a very large canvas.
const MAX_PIXELS: f64 = 8. * 1_024. * 1_024.;
const MAX_SIDE: f64 = 8_192.;
const OPTIONS: &str = r#"{"fontSize":13,"padding":12,"responsive":false,"autoDirection":false,"images":false,"links":false,"maxEdges":512,"outputLimit":1048576}"#;

pub(super) struct Diagram {
	pub image: Arc<RenderImage>,
	pub width: f32,
	pub height: f32,
}

pub(super) fn prepare(
	source: &str,
	renderer: &SvgRenderer,
	theme: Theme,
) -> Result<Diagram, String> {
	let rendered = compile(source, theme)?;
	let svg = renderer.parse_svg(rendered.svg.as_bytes()).map_err(|e| e.to_string())?;
	let image = renderer.render_parsed(&svg, 1.0_f32).map_err(|e| e.to_string())?;

	Ok(Diagram { image, width: rendered.width as f32, height: rendered.height as f32 })
}

fn compile(source: &str, theme: Theme) -> Result<ondine::Rendered, String> {
	let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?;
	let mut rendered =
		ondine::render(source, OPTIONS, now.as_secs_f64() * 1_000.).map_err(|e| e.message)?;

	// GPUI rasterizes SVG at 2x. Reject excessive dimensions before parsing fonts or allocating
	// pixels.
	if !canvas_fits(rendered.width, rendered.height) {
		return Err("Diagram exceeds the desktop image limit".into());
	}

	rendered.svg = svg::desktop(&rendered.svg, theme)?;

	Ok(rendered)
}

fn canvas_fits(width: f64, height: f64) -> bool {
	width > 0.
		&& height > 0.
		&& width * 2. <= MAX_SIDE
		&& height * 2. <= MAX_SIDE
		&& width * height * 4. <= MAX_PIXELS
}

#[cfg(test)]
#[path = "agent_mermaid/tests.rs"]
mod tests;
