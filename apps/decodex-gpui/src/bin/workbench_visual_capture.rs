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
#[path = "../creation_defaults.rs"] mod creation_defaults;
#[allow(dead_code)]
#[path = "../desktop_settings.rs"]
mod desktop_settings;
#[allow(dead_code)]
#[path = "../health_query.rs"]
mod health_query;
#[allow(dead_code)]
#[path = "../history_pager.rs"]
mod history_pager;
#[allow(dead_code)]
#[path = "../native_menu_bar.rs"]
mod native_menu_bar;
#[path = "../panel_preferences.rs"] mod panel_preferences;
#[allow(dead_code)]
#[path = "../settings_surface.rs"]
mod settings_surface;
#[allow(dead_code)]
#[path = "../shell.rs"]
mod shell;
#[allow(dead_code)]
#[path = "../ui_motion.rs"]
mod ui_motion;
#[allow(dead_code)]
#[path = "../ui_theme.rs"]
mod ui_theme;
#[allow(dead_code)]
#[cfg(target_os = "macos")]
use objc2 as _;
use std::path::PathBuf;

use gpui::{AppContext as _, VisualTestAppContext, px, size};

use crate::shell::{Destination, Shell, chief_surface::ChiefSurface};
#[cfg(target_os = "macos")] use {objc2_app_kit as _, objc2_foundation as _};

fn main() -> gpui::Result<()> {
	let output = std::env::var_os("DECODEX_VISUAL_OUTPUT")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from("target/visual-tests/codex-workbench.png"));
	if let Some(parent) = output.parent() {
		std::fs::create_dir_all(parent)?;
	}

	let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
	cx.update(shell::bind_keys);
	let destination = capture_destination();
	let left_sidebar_visible = std::env::var("DECODEX_VISUAL_SIDEBAR").as_deref() != Ok("hidden");
	let inspector_visible = std::env::var("DECODEX_VISUAL_CONTEXT").as_deref() != Ok("hidden");
	let panel_motion = std::env::var("DECODEX_VISUAL_PANEL_MOTION").ok();
	let send_message = std::env::var("DECODEX_VISUAL_CHIEF_SEND").ok();
	let automatic_recap = std::env::var_os("DECODEX_VISUAL_AUTO_RECAP").is_some();
	if (send_message.is_some() || automatic_recap)
		&& std::env::var_os("DECODEX_VISUAL_CHIEF_ROOT").is_none()
	{
		return Err(std::io::Error::other("Command capture requires a disposable root").into());
	}
	let app_ui_mode = std::env::var("DECODEX_VISUAL_APP_UI").ok();
	if app_ui_mode
		.as_ref()
		.is_some_and(|mode| !["confirmation", "unknown"].contains(&mode.as_str()))
	{
		return Err(std::io::Error::other("Unknown App UI capture fixture").into());
	}
	if app_ui_mode.is_some() && std::env::var_os("DECODEX_VISUAL_CHIEF_ROOT").is_some() {
		return Err(std::io::Error::other(
			"App UI layout fixtures cannot be combined with service evidence",
		)
		.into());
	}
	// The explicit root supplies protocol evidence; command probes require their own flags.
	// Never use the installed profile as an implicit screenshot source.
	let service_projection = std::env::var_os("DECODEX_VISUAL_CHIEF_ROOT")
		.map(|root| -> gpui::Result<_> {
			let root = PathBuf::from(root);
			if automatic_recap && root.parent().and_then(|parent| std::fs::read_to_string(parent.join(".decodex-recap-fixture")).ok()).as_deref() != Some("isolated-recap\n") {
				return Err(std::io::Error::other("Automatic recap capture requires the isolated native fixture marker").into());
			}
			let profile = decodex_protocol::ClientProfile::load(&root, None)
				.map_err(|error| std::io::Error::other(format!("capture profile: {error:?}")))?;
			let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
			let client = decodex_protocol::ChiefClient::new(profile.clone());
			let snapshot = runtime
				.block_on(client.query())
				.map_err(|error| std::io::Error::other(format!("capture snapshot: {error:?}")))?;
			let selected = match &snapshot {
				decodex_protocol::ChiefSnapshotResult::Available(snapshot) => {
					std::env::var("DECODEX_VISUAL_CHIEF_WORK")
						.ok()
						.filter(|id| snapshot.work_items.iter().any(|work| &work.id == id))
						.or_else(|| {
							snapshot
								.work_items
								.iter()
								.find(|work| work.parent_goal_id.is_none())
								.map(|work| work.id.clone())
						})
				},
				_ => None,
			};
			let history = selected.as_ref().map(|id| {
				let id = decodex_protocol::EntityId::new(id.clone())
					.expect("validated snapshot identity");
				runtime
					.block_on(client.history(id))
					.unwrap_or(decodex_protocol::ChiefHistoryResult::Unavailable)
			});
			let request = match &snapshot {
				decodex_protocol::ChiefSnapshotResult::Available(snapshot) => snapshot.pending_events.iter().find(|event| Some(&event.work_item_id) == selected.as_ref() && ["permission_pending", "user_input_pending", "server_request_pending"].contains(&event.event_kind.as_str())).map(|event| runtime.block_on(client.request(event.id)).unwrap_or(decodex_protocol::ChiefRequestResult::Unavailable)),
				_ => None,
			};
			let guardian = selected.as_ref().map(|id| {
				runtime.block_on(client.guardian_reviews(decodex_protocol::EntityId::new(id.clone()).expect("validated work"),None))
					.unwrap_or(decodex_protocol::ChiefGuardianReviewsResult::Unavailable)
			});
			std::fs::write(
				output.with_extension("evidence.json"),
				serde_json::to_vec_pretty(
					&serde_json::json!({"source_root": &root, "observed_at_micros": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_micros(), "snapshot": &snapshot, "selected": &selected, "history": &history, "request": &request,"guardian":&guardian}),
				)?,
			)?;
			Ok((snapshot, selected, history, request, guardian, profile))
		})
		.transpose()?;
	let window: gpui::AnyWindowHandle = if let Some(mode) = app_ui_mode {
		cx.open_offscreen_window(size(px(1248.0), px(840.0)), |_, cx| {
			cx.new(|cx| {
				let mut surface = ChiefSurface::new(cx);
				surface.visual_app_ui_confirmation(mode == "unknown", cx);
				surface
			})
		})?
		.into()
	} else if let Some((snapshot, selected, history, request, guardian, profile)) =
		service_projection
	{
		let handle = cx.open_offscreen_window(size(px(1_248.0), px(840.0)), |_, cx| {
			cx.new(|cx| {
				let mut surface =
					ChiefSurface::visual_from_service(snapshot, selected, history, request, cx);
				surface.visual_guardian_reviews(guardian);
				surface
			})
		})?;
		if automatic_recap {
			prove_automatic_recap(&mut cx, handle, profile.clone(), &output)?;
		}
		if let Some(message) = send_message {
			prove_composer_send(&mut cx, handle, profile, &message, &output)?;
		}
		handle.into()
	} else {
		cx.open_offscreen_window(size(px(1_248.0), px(840.0)), |window, cx| {
			cx.new(|cx| {
				Shell::visual_destination(
					destination,
					left_sidebar_visible,
					inspector_visible,
					window,
					cx,
				)
			})
		})?
		.into()
	};
	cx.run_until_parked();
	cx.update_window(window, |_, window, _| window.refresh())?;
	cx.run_until_parked();
	// GPUI element animations use the monotonic wall clock, while async timers
	// use the visual-test dispatcher clock. Render once to start the element
	// animation, then wait on the same clock that drives it.
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
	std::thread::sleep(ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));
	cx.advance_clock(std::time::Duration::from_millis(16));
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
	cx.run_until_parked();
	if let Some(panel_motion) = panel_motion {
		animate_panel_motion(&mut cx, window, &panel_motion)?;
	}

	capture_interactions(&mut cx, window)?;
	let screenshot = cx.capture_screenshot(window)?;
	screenshot.save(&output)?;
	println!("{}", output.display());
	Ok(())
}

fn animate_panel_motion(
	cx: &mut VisualTestAppContext,
	window: gpui::AnyWindowHandle,
	panel_motion: &str,
) -> gpui::Result<()> {
	let keys = match panel_motion {
		"left" => "cmd-e",
		"right" => "cmd-b",
		"both" => "cmd-e cmd-b",
		"graph" => "cmd-j",
		_ => "",
	};
	if !keys.is_empty() {
		cx.simulate_keystrokes(window, keys);
		cx.run_until_parked();
		// Render once at the new generation to start its animation, then wait
		// until approximately the midpoint before taking the evidence frame.
		cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
		std::thread::sleep(ui_theme::MOTION_PANEL / 2);
		cx.advance_clock(std::time::Duration::from_millis(16));
		cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
		cx.run_until_parked();
	}
	Ok(())
}

// Sample dismissal over the real composer, including its Live button.
fn capture_status_dismissal(
	cx: &mut VisualTestAppContext,
	window: gpui::AnyWindowHandle,
) -> gpui::Result<()> {
	let Some(delay) = std::env::var("DECODEX_VISUAL_STATUS_CLOSE_MS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
	else {
		return Ok(());
	};
	cx.simulate_click(window, gpui::point(px(1200.), px(815.)), Default::default());
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
	std::thread::sleep(std::time::Duration::from_millis(delay));
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
	Ok(())
}

// Exercise the AppKit responder boundary, which headless GPUI tests cannot cover.
#[cfg(target_os = "macos")]
fn verify_native_composer_focus(
	cx: &mut VisualTestAppContext,
	window: gpui::AnyWindowHandle,
) -> gpui::Result<()> {
	if std::env::var_os("DECODEX_VISUAL_NATIVE_INPUT_FOCUS").is_none() {
		return Ok(());
	}
	use objc2_app_kit::{NSResponder, NSView};
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	cx.update_window(window, |_, window, _| {
		let handle =
			HasWindowHandle::window_handle(window).expect("offscreen window has a native handle");
		let RawWindowHandle::AppKit(handle) = handle.as_raw() else { panic!("AppKit required") };
		let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
		assert!(
			view.window()
				.expect("capture view belongs to a native window")
				.makeFirstResponder(None)
		);
	})?;
	cx.simulate_click(window, gpui::point(px(500.), px(790.)), Default::default());
	cx.update_window(window, |_, window, _| {
		let handle =
			HasWindowHandle::window_handle(window).expect("offscreen window has a native handle");
		let RawWindowHandle::AppKit(handle) = handle.as_raw() else { panic!("AppKit required") };
		let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
		let responder = view
			.window()
			.expect("capture view belongs to a native window")
			.firstResponder()
			.expect("composer acquired native focus");
		assert!(
			std::ptr::eq::<NSResponder>(&*responder, view.as_ref()),
			"Editor click must restore the native GPUI text client"
		);
	})?;
	Ok(())
}

fn capture_interactions(
	cx: &mut VisualTestAppContext,
	window: gpui::AnyWindowHandle,
) -> gpui::Result<()> {
	#[cfg(target_os = "macos")]
	verify_native_composer_focus(cx, window)?;
	capture_status_dismissal(cx, window)?;
	let Ok(value) = std::env::var("DECODEX_VISUAL_HOVER") else {
		return Ok(());
	};
	let Some((x, y)) = value.split_once(',') else {
		return Ok(());
	};
	let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>()) else {
		return Ok(());
	};
	cx.simulate_mouse_move(window, gpui::point(px(x), px(y)), None, Default::default());
	cx.advance_clock(std::time::Duration::from_secs(1));
	cx.run_until_parked();
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
	std::thread::sleep(ui_theme::MOTION_PANEL + std::time::Duration::from_millis(40));
	cx.update_window(window, |_, window, cx| {
		window.refresh();
		window.draw(cx).clear();
	})?;
	Ok(())
}

fn capture_destination() -> Destination {
	match std::env::var("DECODEX_VISUAL_DESTINATION").as_deref() {
		Ok("chief") => Destination::Chief,
		Ok("accounts") => Destination::Accounts,
		Ok("health") => Destination::Health,
		Ok("settings") => Destination::Settings,
		_ => Destination::Conversations,
	}
}

fn prove_composer_send(
	cx: &mut VisualTestAppContext,
	handle: gpui::WindowHandle<ChiefSurface>,
	profile: decodex_protocol::ClientProfile,
	message: &str,
	output: &std::path::Path,
) -> gpui::Result<()> {
	cx.update_window(handle.into(), |view, window, cx| {
		view.downcast::<ChiefSurface>()
			.expect("Chief capture root")
			.update(cx, |surface, cx| surface.visual_prepare_send(profile, message, window, cx));
		window.draw(cx).clear();
	})?;
	cx.simulate_keystrokes(handle.into(), "enter");
	for _ in 0..40 {
		cx.run_until_parked();
		std::thread::sleep(std::time::Duration::from_millis(500));
		cx.update_window(handle.into(), |view, _, cx| {
			view.downcast::<ChiefSurface>()
				.expect("Chief capture root")
				.update(cx, ChiefSurface::refresh)
		})?;
		cx.run_until_parked();
		let evidence = cx.update_window(handle.into(), |view, _, cx| {
			view.downcast::<ChiefSurface>()
				.expect("Chief capture root")
				.update(cx, |surface, cx| surface.visual_send_evidence(cx))
		})?;
		std::fs::write(
			output.with_extension("send.json"),
			serde_json::to_vec_pretty(
				&serde_json::json!({"submitted_message":message,"interaction":"ComposerInput Enter","result":evidence}),
			)?,
		)?;
		if evidence["uncertain"] == true {
			break;
		}
		let answered = evidence["history"][1]["entries"].as_array().is_some_and(|entries| {
			entries.iter().any(|entry| {
				entry["kind"] == "assistant"
					&& entry["text"].as_str().is_some_and(|text| text.contains("UI_READY"))
			})
		});
		if evidence["feedback"].as_str().is_some_and(|text| text.starts_with("Accepted by service"))
			&& evidence["draft"] == ""
			&& answered
		{
			break;
		}
	}
	Ok(())
}

fn prove_automatic_recap(
	cx: &mut VisualTestAppContext,
	handle: gpui::WindowHandle<ChiefSurface>,
	profile: decodex_protocol::ClientProfile,
	output: &std::path::Path,
) -> gpui::Result<()> {
	// This capture intentionally combines deterministic UI scheduling with real service I/O.
	cx.background_executor.allow_parking();
	eprintln!("Automatic recap fixture: initial draw");
	cx.update_window(handle.into(), |_, window, cx| window.draw(cx).clear())?;
	cx.update_window(handle.into(), |view, _, cx| {
		view.downcast::<ChiefSurface>()
			.expect("Chief capture root")
			.update(cx, |s, cx| s.visual_begin_automatic_recap(profile, cx));
	})?;
	eprintln!("Automatic recap fixture: driver armed");
	for _ in 0..60 {
		cx.run_until_parked();
		let evidence = cx.update_window(handle.into(), |view, _, cx| {
			view.downcast::<ChiefSurface>()
				.expect("Chief capture root")
				.update(cx, ChiefSurface::visual_automatic_recap_evidence)
		})?;
		std::fs::write(output.with_extension("recap.json"), serde_json::to_vec_pretty(&evidence)?)?;
		if evidence["automatic"] == true && evidence["state"]["phase"] == "ready" {
			return Ok(());
		}
		std::thread::sleep(std::time::Duration::from_millis(250));
	}
	Err(std::io::Error::other("Automatic recap did not reach Ready in the isolated capture").into())
}
