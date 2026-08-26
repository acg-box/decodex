//! Deterministic native screenshot capture for the Factory design review.

#[allow(dead_code)]
#[path = "../composer_input.rs"]
mod composer_input;
#[allow(dead_code)]
#[path = "../factory_surface.rs"]
mod factory_surface;
#[allow(dead_code)]
#[path = "../program_graph.rs"]
mod program_graph;
#[allow(dead_code)]
#[path = "../programs.rs"]
mod programs;
#[allow(dead_code)]
#[path = "../ui_theme.rs"]
mod ui_theme;
#[allow(dead_code)]
#[path = "../work_items.rs"]
mod work_items;

#[cfg(target_os = "macos")] use objc2 as _;
use std::path::PathBuf;

use gpui::{AppContext as _, VisualTestAppContext, px, size};

use crate::{factory_surface::FactorySurface, programs::Programs, work_items::WorkItems};

#[derive(Clone, Copy)]
enum VisualScenario {
	Development,
	PaperInvestment,
}

impl VisualScenario {
	fn from_environment() -> gpui::Result<Self> {
		match std::env::var("DECODEX_VISUAL_SCENARIO") {
			Ok(value) if value == "development" => Ok(Self::Development),
			Ok(value) if value == "paper-investment" => Ok(Self::PaperInvestment),
			Err(std::env::VarError::NotPresent) => Ok(Self::Development),
			Ok(value) => Err(std::io::Error::new(
				std::io::ErrorKind::InvalidInput,
				format!(
					"unsupported DECODEX_VISUAL_SCENARIO {value:?}; expected development or paper-investment"
				),
			)
			.into()),
			Err(error) => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, error).into()),
		}
	}

	fn default_output(self) -> PathBuf {
		PathBuf::from(match self {
			Self::Development => "target/visual-tests/program-graph-development-three-cycle.png",
			Self::PaperInvestment => "target/visual-tests/program-graph-paper-investment.png",
		})
	}

	fn programs(self) -> Programs {
		match self {
			Self::Development => Programs::visual_development_three_cycle(),
			Self::PaperInvestment => Programs::visual_paper_investment(),
		}
	}
}

fn main() -> gpui::Result<()> {
	let scenario = VisualScenario::from_environment()?;
	let output = std::env::var_os("DECODEX_VISUAL_OUTPUT")
		.map(PathBuf::from)
		.unwrap_or_else(|| scenario.default_output());
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
			surface.bind_programs(scenario.programs(), cx);
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
