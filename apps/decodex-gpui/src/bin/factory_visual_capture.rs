//! Deterministic native screenshot capture for the Factory design review.

#[allow(dead_code)]
#[path = "../composer_input.rs"]
mod composer_input;
#[allow(dead_code)]
#[path = "../factory_surface.rs"]
mod factory_surface;
#[allow(dead_code)]
#[path = "../programs.rs"]
mod programs;
#[allow(dead_code)]
#[path = "../ui_theme.rs"]
mod ui_theme;
#[allow(dead_code)]
#[path = "../work_items.rs"]
mod work_items;

use libc as _;
use objc2_app_kit as _;
use objc2_foundation as _;
use serde as _;
use serde_json as _;
#[cfg(test)] use tempfile as _;

use std::path::PathBuf;

use gpui::{AppContext as _, VisualTestAppContext, px, size};

use crate::{factory_surface::FactorySurface, programs::Programs, work_items::WorkItems};

fn main() -> gpui::Result<()> {
	let output = std::env::var_os("DECODEX_VISUAL_OUTPUT")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from("target/visual-tests/factory-operate.png"));
	if let Some(parent) = output.parent() {
		std::fs::create_dir_all(parent)?;
	}

	let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
	cx.update(|cx| {
		composer_input::bind_keys(cx);
		factory_surface::bind_keys(cx);
	});
	let window = cx.open_offscreen_window(size(px(1_490.0), px(1_092.0)), |_, cx| {
		cx.new(|cx| {
			let mut surface = FactorySurface::new(cx);
			surface.bind_work_items(WorkItems::visual_no_projects(), cx);
			surface.bind_programs(Programs::visual_closed_cycle(), cx);
			surface
		})
	})?;
	cx.run_until_parked();
	cx.update_window(window.into(), |_, window, _| window.refresh())?;
	cx.run_until_parked();
	cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
	std::thread::sleep(ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));
	cx.advance_clock(std::time::Duration::from_millis(16));
	cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
	cx.update_window(window.into(), |_, window, _| window.refresh())?;
	cx.run_until_parked();

	let screenshot = cx.capture_screenshot(window.into())?;
	screenshot.save(&output)?;
	println!("{}", output.display());
	Ok(())
}
