//! Deterministic native screenshot capture for the Codex Workbench design review.

#[allow(dead_code)]
#[path = "../account_login.rs"]
mod account_login;
#[allow(dead_code)]
#[path = "../account_profile.rs"]
mod account_profile;
#[allow(dead_code)]
#[path = "../accounts.rs"]
mod accounts;
#[allow(dead_code)]
#[path = "../client_cache.rs"]
mod client_cache;
#[allow(dead_code)]
#[path = "../client_lifecycle.rs"]
mod client_lifecycle;
#[allow(dead_code)]
#[path = "../composer_input.rs"]
mod composer_input;
#[allow(dead_code)]
#[path = "../conversations.rs"]
mod conversations;
#[allow(dead_code)]
#[path = "../desktop_settings.rs"]
mod desktop_settings;
#[allow(dead_code)]
#[path = "../factory_surface.rs"]
mod factory_surface;
#[allow(dead_code)]
#[path = "../health_query.rs"]
mod health_query;
#[allow(dead_code)]
#[path = "../history_pager.rs"]
mod history_pager;
#[allow(dead_code)]
#[path = "../native_menu_bar.rs"]
mod native_menu_bar;
#[allow(dead_code)]
#[path = "../program_graph.rs"]
mod program_graph;
#[allow(dead_code)]
#[path = "../programs.rs"]
mod programs;
#[allow(dead_code)]
#[path = "../settings_surface.rs"]
mod settings_surface;
#[allow(dead_code)]
#[path = "../shell.rs"]
mod shell;
#[allow(dead_code)]
#[path = "../ui_theme.rs"]
mod ui_theme;
#[allow(dead_code)]
#[cfg(target_os = "macos")]
use objc2 as _;
use std::path::PathBuf;

use gpui::{AppContext as _, VisualTestAppContext, px, size};

use crate::shell::{Destination, Shell};

fn main() -> gpui::Result<()> {
	let output = std::env::var_os("DECODEX_VISUAL_OUTPUT")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from("target/visual-tests/codex-workbench.png"));
	if let Some(parent) = output.parent() {
		std::fs::create_dir_all(parent)?;
	}

	let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
	cx.update(shell::bind_keys);
	let destination = match std::env::var("DECODEX_VISUAL_DESTINATION").as_deref() {
		Ok("factory") => Destination::Factory,
		Ok("accounts") => Destination::Accounts,
		Ok("health") => Destination::Health,
		Ok("settings") => Destination::Settings,
		_ => Destination::Conversations,
	};
	let left_sidebar_visible = std::env::var("DECODEX_VISUAL_SIDEBAR").as_deref() != Ok("hidden");
	let inspector_visible = std::env::var("DECODEX_VISUAL_CONTEXT").as_deref() != Ok("hidden");
	let panel_motion = std::env::var("DECODEX_VISUAL_PANEL_MOTION").ok();
	let window = cx.open_offscreen_window(size(px(1_248.0), px(840.0)), |window, cx| {
		cx.new(|cx| {
			Shell::visual_destination(
				destination,
				left_sidebar_visible,
				inspector_visible,
				window,
				cx,
			)
		})
	})?;
	cx.run_until_parked();
	cx.update_window(window.into(), |_, window, _| window.refresh())?;
	cx.run_until_parked();
	// GPUI element animations use the monotonic wall clock, while async timers
	// use the visual-test dispatcher clock. Render once to start the element
	// animation, then wait on the same clock that drives it.
	cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
	std::thread::sleep(ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));
	cx.advance_clock(std::time::Duration::from_millis(16));
	cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
	cx.run_until_parked();
	if let Some(panel_motion) = panel_motion {
		let keys = match panel_motion.as_str() {
			"left" => "cmd-b",
			"right" => "cmd-shift-b",
			"both" => "cmd-b cmd-shift-b",
			_ => "",
		};
		if !keys.is_empty() {
			cx.simulate_keystrokes(window.into(), keys);
			cx.run_until_parked();
			// Render once at the new generation to start its animation, then wait
			// until approximately the midpoint before taking the evidence frame.
			cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
			std::thread::sleep(ui_theme::MOTION_PANEL / 2);
			cx.advance_clock(std::time::Duration::from_millis(16));
			cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
			cx.run_until_parked();
		}
	}

	let screenshot = cx.capture_screenshot(window.into())?;
	screenshot.save(&output)?;
	println!("{}", output.display());
	Ok(())
}
