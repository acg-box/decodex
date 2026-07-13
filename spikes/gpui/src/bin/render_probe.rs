use std::{collections::HashSet, time::Instant};

use decodex_gpui_spike::{WorkspaceSpike, text_input};
use gpui::{AppContext, VisualTestAppContext};

fn main() {
	let output = std::env::args().nth(1).expect("screenshot output path");
	let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
	cx.update(text_input::bind_keys);
	let window = cx
		.open_offscreen_window_default(|window, cx| cx.new(|cx| WorkspaceSpike::new(window, cx)))
		.expect("open offscreen Metal window");
	cx.run_until_parked();
	let workspace = window.root(&mut cx).expect("workspace root");
	let frames = 120usize;
	let mut frame_micros = Vec::with_capacity(frames);
	for _ in 0..frames {
		let started = Instant::now();
		workspace.update(&mut cx, |_, cx| cx.notify());
		cx.run_until_parked();
		frame_micros.push(started.elapsed().as_micros());
	}
	frame_micros.sort_unstable();
	let p50 = frame_micros[frames / 2];
	let p95 = frame_micros[frames * 95 / 100];
	let max = frame_micros[frames - 1];
	let screenshot = cx.capture_screenshot(window.into()).expect("capture Metal texture");
	let colors = screenshot.pixels().map(|pixel| pixel.0).collect::<HashSet<_>>();
	assert!(colors.len() > 16, "expected rendered content, saw {} colors", colors.len());
	screenshot.save(&output).expect("save captured Metal texture");
	println!(
		"{{\"width\":{},\"height\":{},\"unique_rgba_colors\":{},\"frames\":{frames},\"p50_micros\":{p50},\"p95_micros\":{p95},\"max_micros\":{max},\"output\":\"{}\"}}",
		screenshot.width(),
		screenshot.height(),
		colors.len(),
		output
	);
}
