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
#[path = "../ui_loading.rs"]
mod ui_loading;
#[path = "../ui_motion.rs"] mod ui_motion;
#[allow(dead_code)]
#[path = "../ui_preferences.rs"]
mod ui_preferences;
#[allow(dead_code)]
#[path = "../ui_scroll.rs"]
mod ui_scroll;
#[allow(dead_code)]
#[path = "../ui_theme.rs"]
mod ui_theme;
#[path = "../ui_working.rs"] mod ui_working;

use std::{
	env, fs,
	io::Error,
	path::{Path, PathBuf},
	thread,
	time::Duration,
};

use gpui::{
	self, AnyWindowHandle, AppContext as _, Result, ScrollDelta, ScrollWheelEvent,
	VisualTestAppContext, WindowHandle,
};
#[allow(dead_code)]
#[cfg(target_os = "macos")]
use objc2 as _;
#[cfg(target_os = "macos")] use objc2_app_kit::{NSResponder, NSView};
#[cfg(target_os = "macos")] use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::Value;
use tokio::runtime::Builder;
#[cfg(target_os = "macos")] use {objc2_app_kit as _, objc2_foundation as _};

use crate::shell::{Destination, Shell, agent_surface::AgentSurface};
use decodex_protocol::{
	AgentClient, AgentMediaRequest, AgentSnapshotResult, AgentSteerIdentity, ClientFailure,
	EntityId,
};
use ui_theme::MOTION_PANEL;

type ServiceProjection = (
	AgentSnapshotResult,
	Option<String>,
	Option<decodex_protocol::AgentHistoryResult>,
	Option<decodex_protocol::AgentRequestResult>,
	Option<decodex_protocol::AgentGuardianReviewsResult>,
	decodex_protocol::ClientProfile,
);

fn main() -> Result<()> {
	let output = env::var_os("DECODEX_VISUAL_OUTPUT")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from("target/visual-tests/codex-workbench.png"));

	if let Some(parent) = output.parent() {
		fs::create_dir_all(parent)?;
	}

	let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));

	cx.update(shell::bind_keys);

	let destination = capture_destination();
	let left_sidebar_visible = env::var("DECODEX_VISUAL_SIDEBAR").as_deref() != Ok("hidden");
	let inspector_visible = env::var("DECODEX_VISUAL_CONTEXT").as_deref() != Ok("hidden");
	let panel_motion = env::var("DECODEX_VISUAL_PANEL_MOTION").ok();
	let send_message = env::var("DECODEX_VISUAL_AGENT_SEND").ok();
	let steer_receipt = env::var("DECODEX_VISUAL_AGENT_STEER_RECEIPT").ok();
	let live_media = env::var_os("DECODEX_VISUAL_MEDIA").is_some();
	let automatic_recap = env::var_os("DECODEX_VISUAL_AUTO_RECAP").is_some();

	if (send_message.is_some() || steer_receipt.is_some() || automatic_recap || live_media)
		&& env::var_os("DECODEX_VISUAL_AGENT_ROOT").is_none()
	{
		return Err(Error::other("Command capture requires a disposable root").into());
	}

	let integrations = layout_fixtures()?;
	// The explicit root supplies protocol evidence; command probes require their own flags.
	// Never use the installed profile as an implicit screenshot source.
	let service_projection = env::var_os("DECODEX_VISUAL_AGENT_ROOT")
		.map(|root| {
			read_service_projection(PathBuf::from(root), &output, automatic_recap || live_media)
		})
		.transpose()?;
	let window: AnyWindowHandle = if integrations {
		cx.open_offscreen_window(gpui::size(gpui::px(1_180.0), gpui::px(1_400.0)), |_, cx| {
			cx.new(|cx| {
				let mut surface = AgentSurface::new(cx);

				surface.visual_integrations(cx);

				surface
			})
		})?
		.into()
	} else if let Some((snapshot, selected, history, request, guardian, profile)) =
		service_projection
	{
		let handle =
			cx.open_offscreen_window(gpui::size(gpui::px(1_248.0), gpui::px(840.0)), |_, cx| {
				cx.new(|cx| {
					let mut surface =
						AgentSurface::visual_from_service(snapshot, selected, history, request, cx);

					surface.visual_guardian_reviews(guardian);

					surface
				})
			})?;

		if live_media {
			let root =
				PathBuf::from(env::var_os("DECODEX_VISUAL_AGENT_ROOT").expect("explicit root"));

			prove_media(&mut cx, handle, profile.clone(), &root, &output)?;
		}
		if automatic_recap {
			prove_automatic_recap(&mut cx, handle, profile.clone(), &output)?;
		}

		if let Some(message) = send_message {
			prove_composer_send(&mut cx, handle, profile.clone(), &message, &output)?;
		}
		if let Some(identity) = steer_receipt {
			prove_steer_receipt(&mut cx, handle, profile, &identity, &output)?;
		}

		handle.into()
	} else {
		cx.open_offscreen_window(gpui::size(gpui::px(1_248.0), gpui::px(840.0)), |window, cx| {
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

	thread::sleep(MOTION_PANEL + Duration::from_millis(40));

	cx.advance_clock(Duration::from_millis(16));
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
	cx.run_until_parked();

	if let Some(panel_motion) = panel_motion {
		animate_panel_motion(&mut cx, window, &panel_motion)?;
	}

	if integrations {
		capture_integrations(&mut cx, window, &output)?;
	}

	capture_interactions(&mut cx, window)?;

	let screenshot = cx.capture_screenshot(window)?;

	screenshot.save(&output)?;

	println!("{}", output.display());

	Ok(())
}

fn capture_integrations(
	cx: &mut VisualTestAppContext,
	window: AnyWindowHandle,
	output: &Path,
) -> Result<()> {
	cx.capture_screenshot(window)?.save(output.with_extension("top.png"))?;
	// The isolated 1180-pixel-wide fixture puts the status viewport below its controls.
	cx.simulate_event(
		window,
		ScrollWheelEvent {
			position: gpui::point(gpui::px(450.), gpui::px(620.)),
			delta: ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-460.))),
			..Default::default()
		},
	);
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;

	Ok(())
}

fn layout_fixtures() -> Result<bool> {
	let integrations = env::var_os("DECODEX_VISUAL_INTEGRATIONS").is_some();

	if integrations && env::var_os("DECODEX_VISUAL_AGENT_ROOT").is_some() {
		return Err(Error::other("Integration layout fixture cannot use a service source").into());
	}

	Ok(integrations)
}

fn read_service_projection(
	root: PathBuf,
	output: &Path,
	require_fixture: bool,
) -> Result<ServiceProjection> {
	if require_fixture
		&& root
			.parent()
			.and_then(|parent| fs::read_to_string(parent.join(".decodex-recap-fixture")).ok())
			.as_deref()
			!= Some("isolated-recap\n")
	{
		return Err(Error::other(
			"Automatic recap capture requires the isolated native fixture marker",
		)
		.into());
	}

	let profile = decodex_protocol::ClientProfile::load(&root, None)
		.map_err(|error| Error::other(format!("capture profile: {error:?}")))?;
	let runtime = Builder::new_current_thread().enable_all().build()?;
	let client = AgentClient::new(profile.clone());
	let snapshot = runtime
		.block_on(client.query())
		.map_err(|error| Error::other(format!("capture snapshot: {error:?}")))?;
	let selected = match &snapshot {
		AgentSnapshotResult::Available(snapshot) => env::var("DECODEX_VISUAL_AGENT_WORK")
			.ok()
			.filter(|id| snapshot.work_items.iter().any(|work| &work.id == id))
			.or_else(|| {
				snapshot
					.work_items
					.iter()
					.find(|work| work.parent_goal_id.is_none())
					.map(|work| work.id.clone())
			}),
		_ => None,
	};
	let history = selected.as_ref().map(|id| {
		let id = EntityId::new(id.clone()).expect("validated snapshot identity");

		runtime
			.block_on(client.history(id))
			.unwrap_or(decodex_protocol::AgentHistoryResult::Unavailable)
	});
	let request = match &snapshot {
		AgentSnapshotResult::Available(snapshot) => snapshot
			.pending_events
			.iter()
			.find(|event| {
				Some(&event.work_item_id) == selected.as_ref()
					&& ["permission_pending", "user_input_pending", "server_request_pending"]
						.contains(&event.event_kind.as_str())
			})
			.map(|event| {
				runtime
					.block_on(client.request(event.id))
					.unwrap_or(decodex_protocol::AgentRequestResult::Unavailable)
			}),
		_ => None,
	};
	let guardian = selected.as_ref().map(|id| {
		runtime
			.block_on(
				client.guardian_reviews(EntityId::new(id.clone()).expect("validated work"), None),
			)
			.unwrap_or(decodex_protocol::AgentGuardianReviewsResult::Unavailable)
	});

	fs::write(
		output.with_extension("evidence.json"),
		serde_json::to_vec_pretty(
			&serde_json::json!({"source_root": &root, "observed_at_micros": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_micros(), "snapshot": &snapshot, "selected": &selected, "history": &history, "request": &request,"guardian":&guardian}),
		)?,
	)?;

	Ok((snapshot, selected, history, request, guardian, profile))
}

fn animate_panel_motion(
	cx: &mut VisualTestAppContext,
	window: AnyWindowHandle,
	panel_motion: &str,
) -> Result<()> {
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

		thread::sleep(MOTION_PANEL / 2);

		cx.advance_clock(Duration::from_millis(16));
		cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
		cx.run_until_parked();
	}

	Ok(())
}

// Sample dismissal over the real composer, including its Live button.
fn capture_status_dismissal(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Result<()> {
	let Some(delay) =
		env::var("DECODEX_VISUAL_STATUS_CLOSE_MS").ok().and_then(|value| value.parse::<u64>().ok())
	else {
		return Ok(());
	};

	cx.simulate_click(window, gpui::point(gpui::px(1_200.), gpui::px(815.)), Default::default());
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;

	thread::sleep(Duration::from_millis(delay));

	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;

	Ok(())
}

// Exercise the AppKit responder boundary, which headless GPUI tests cannot cover.
#[cfg(target_os = "macos")]
fn verify_native_composer_focus(
	cx: &mut VisualTestAppContext,
	window: AnyWindowHandle,
) -> Result<()> {
	if std::env::var_os("DECODEX_VISUAL_NATIVE_INPUT_FOCUS").is_none() {
		return Ok(());
	}

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

	cx.simulate_click(window, gpui::point(gpui::px(500.), gpui::px(790.)), Default::default());

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

fn capture_interactions(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Result<()> {
	#[cfg(target_os = "macos")]
	verify_native_composer_focus(cx, window)?;
	capture_status_dismissal(cx, window)?;

	let Ok(value) = env::var("DECODEX_VISUAL_HOVER") else {
		return Ok(());
	};
	let Some((x, y)) = value.split_once(',') else {
		return Ok(());
	};
	let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>()) else {
		return Ok(());
	};

	cx.simulate_mouse_move(window, gpui::point(gpui::px(x), gpui::px(y)), None, Default::default());
	cx.advance_clock(Duration::from_secs(1));
	cx.run_until_parked();
	cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;

	thread::sleep(MOTION_PANEL + Duration::from_millis(40));

	cx.update_window(window, |_, window, cx| {
		window.refresh();
		window.draw(cx).clear();
	})?;

	Ok(())
}

fn capture_destination() -> Destination {
	match env::var("DECODEX_VISUAL_DESTINATION").as_deref() {
		Ok("agent") => Destination::Agent,
		Ok("accounts") => Destination::Accounts,
		Ok("health") => Destination::Health,
		Ok("settings") => Destination::Settings,
		_ => Destination::Conversations,
	}
}

fn prove_composer_send(
	cx: &mut VisualTestAppContext,
	handle: WindowHandle<AgentSurface>,
	profile: decodex_protocol::ClientProfile,
	message: &str,
	output: &Path,
) -> Result<()> {
	cx.background_executor.allow_parking();

	let before = cx.update_window(handle.into(), |view, window, cx| {
		let evidence = view.downcast::<AgentSurface>().expect("Agent capture root").update(
			cx,
			|surface, cx| {
				surface.visual_prepare_send(profile, message, window, cx);

				surface.visual_send_evidence(cx)
			},
		);

		window.draw(cx).clear();

		evidence
	})?;

	cx.simulate_keystrokes(handle.into(), "enter");

	for _ in 0..40 {
		cx.run_until_parked();

		thread::sleep(Duration::from_millis(500));

		cx.update_window(handle.into(), |view, _, cx| {
			view.downcast::<AgentSurface>()
				.expect("Agent capture root")
				.update(cx, AgentSurface::refresh)
		})?;
		cx.run_until_parked();

		let evidence = cx.update_window(handle.into(), |view, _, cx| {
			view.downcast::<AgentSurface>()
				.expect("Agent capture root")
				.update(cx, |surface, cx| surface.visual_send_evidence(cx))
		})?;

		fs::write(
			output.with_extension("send.json"),
			serde_json::to_vec_pretty(
				&serde_json::json!({"submitted_message":message,"interaction":"ComposerInput Enter","before":before,"result":evidence}),
			)?,
		)?;

		if evidence["uncertain"] == true {
			return Err(Error::other("Composer send acceptance is unknown").into());
		}
		if composer_send_answered(&before, &evidence, message) {
			return Ok(());
		}
	}

	Err(Error::other("Composer send did not receive its UI_READY reply").into())
}

fn prove_automatic_recap(
	cx: &mut VisualTestAppContext,
	handle: WindowHandle<AgentSurface>,
	profile: decodex_protocol::ClientProfile,
	output: &Path,
) -> Result<()> {
	// This capture intentionally combines deterministic UI scheduling with real service I/O.
	cx.background_executor.allow_parking();

	eprintln!("Automatic recap fixture: initial draw");

	cx.update_window(handle.into(), |_, window, cx| window.draw(cx).clear())?;
	cx.update_window(handle.into(), |view, _, cx| {
		view.downcast::<AgentSurface>()
			.expect("Agent capture root")
			.update(cx, |s, cx| s.visual_begin_automatic_recap(profile, cx));
	})?;

	eprintln!("Automatic recap fixture: driver armed");

	for _ in 0..60 {
		cx.run_until_parked();

		let evidence = cx.update_window(handle.into(), |view, _, cx| {
			view.downcast::<AgentSurface>()
				.expect("Agent capture root")
				.update(cx, AgentSurface::visual_automatic_recap_evidence)
		})?;

		fs::write(output.with_extension("recap.json"), serde_json::to_vec_pretty(&evidence)?)?;

		if evidence["automatic"] == true && evidence["state"]["phase"] == "ready" {
			return Ok(());
		}

		thread::sleep(Duration::from_millis(250));
	}

	Err(Error::other("Automatic recap did not reach Ready in the isolated capture").into())
}

fn prove_media(
	cx: &mut VisualTestAppContext,
	handle: WindowHandle<AgentSurface>,
	profile: decodex_protocol::ClientProfile,
	root: &Path,
	output: &Path,
) -> Result<()> {
	let request: AgentMediaRequest = serde_json::from_slice(&fs::read(
		root.parent().expect("fixture parent").join("media-source.json"),
	)?)?;
	let runtime = Builder::new_current_thread().enable_all().build()?;
	let client = AgentClient::new(profile.clone());
	let (history, timeline) = runtime
		.block_on(async {
			Ok::<_, ClientFailure>((
				client.history(request.work_id.clone()).await?,
				client.timeline(request.work_id.clone(), request.thread_id.clone(), None).await?,
			))
		})
		.map_err(|error| Error::other(format!("media source: {error:?}")))?;

	cx.background_executor.allow_parking();
	cx.update_window(handle.into(), |view, _, cx| {
		view.downcast::<AgentSurface>().expect("Agent capture").update(cx, |surface, cx| {
			surface.visual_preview_native_media(profile, request, history, timeline, cx)
		})
	})?
	.map_err(Error::other)?;

	for _ in 0..80 {
		cx.run_until_parked();

		let evidence = cx.update_window(handle.into(), |view, window, cx| {
			window.draw(cx).clear();

			view.downcast::<AgentSurface>().expect("Agent capture").read(cx).visual_media_evidence()
		})?;

		fs::write(output.with_extension("media.json"), serde_json::to_vec_pretty(&evidence)?)?;

		if evidence["imageLoaded"] == true {
			return Ok(());
		}

		if let Some(notice) =
			evidence["notice"].as_str().filter(|notice| *notice != "Loading image…")
		{
			return Err(Error::other(notice.to_owned()).into());
		}

		thread::sleep(Duration::from_millis(100));

		cx.advance_clock(Duration::from_millis(100));
	}

	Err(Error::other("Native media preview did not load").into())
}

fn prove_steer_receipt(
	cx: &mut VisualTestAppContext,
	handle: WindowHandle<AgentSurface>,
	profile: decodex_protocol::ClientProfile,
	identity: &str,
	output: &Path,
) -> Result<()> {
	let identity: AgentSteerIdentity = serde_json::from_str(identity)?;
	let before = cx.update_window(handle.into(), |view, window, cx| {
		let evidence = view.downcast::<AgentSurface>().expect("Agent capture root").update(
			cx,
			|surface, cx| {
				surface.visual_uncertain_steer(profile, identity.clone(), cx);

				surface.visual_send_evidence(cx)
			},
		);

		window.draw(cx).clear();

		evidence
	})?;

	if before["uncertain"] != true {
		return Err(Error::other("fixture uncertainty was not established").into());
	}

	for _ in 0..40 {
		cx.update_window(handle.into(), |view, _, cx| {
			view.downcast::<AgentSurface>()
				.expect("Agent capture root")
				.update(cx, AgentSurface::refresh)
		})?;
		cx.run_until_parked();

		thread::sleep(Duration::from_millis(250));

		cx.run_until_parked();

		let after = cx.update_window(handle.into(), |view, window, cx| {
			window.draw(cx).clear();

			view.downcast::<AgentSurface>()
				.expect("Agent capture root")
				.update(cx, |surface, cx| surface.visual_send_evidence(cx))
		})?;

		if after["uncertain"] == false {
			if after["draft"] != "Later draft retained." {
				return Err(Error::other("receipt overwrote the later draft").into());
			}

			fs::write(
				output.with_extension("steer.json"),
				serde_json::to_vec_pretty(
					&serde_json::json!({"identity":identity,"before":before,"after":after}),
				)?,
			)?;

			return Ok(());
		}
	}

	Err(Error::other("exact receipt did not settle UI uncertainty").into())
}

fn composer_send_answered(before: &Value, evidence: &Value, message: &str) -> bool {
	if evidence["uncertain"] != false || evidence["sending"] != false || evidence["draft"] != "" {
		return false;
	}

	let (Some(owner), Some(previous), Some(entries)) = (
		before["history"][0].as_str(),
		before["history"][1]["entries"].as_array(),
		evidence["history"][1]["entries"].as_array(),
	) else {
		return false;
	};

	if evidence["history"][0] != owner {
		return false;
	}

	entries
		.iter()
		.filter(|entry| {
			entry["kind"] == "user"
				&& entry["text"] == message
				&& entry["id"].as_i64().is_some()
				&& !previous.iter().any(|old| old["id"] == entry["id"])
		})
		.any(|prompt| {
			let turn = prompt["turn_id"]
				.as_str()
				.or_else(|| prompt["receipt"]["delivered_turn_id"].as_str());

			turn.filter(|turn| !turn.is_empty()).is_some_and(|turn| {
				entries.iter().any(|entry| {
					entry["kind"] == "assistant"
						&& entry["id"].as_i64().is_some()
						&& !previous.iter().any(|old| old["id"] == entry["id"])
						&& entry["turn_id"] == turn
						&& entry["text"].as_str().is_some_and(|text| text.contains("UI_READY"))
				})
			})
		})
}

#[cfg(test)]
mod capture_send_tests {
	use serde_json::{self, Value};

	fn evidence() -> Value {
		serde_json::json!({"draft":"", "sending":false, "uncertain":false, "feedback":"",
		"history":["manager", {"outcome":"available", "entries":[
			{"id":1,"kind":"user","text":"Reply UI_READY", "turn_id":"turn"},
			{"id":2,"kind":"assistant","text":"UI_READY", "turn_id":"turn"}
		]}]})
	}

	#[test]
	fn capture_send_accepts_current_history_without_obsolete_feedback() {
		let before =
			serde_json::json!({"history":["manager", {"outcome":"available", "entries":[]}]});

		assert!(crate::composer_send_answered(&before, &evidence(), "Reply UI_READY"));

		let mut after = evidence();

		after["history"][1]["entries"][0]["turn_id"] = Value::Null;
		after["history"][1]["entries"][0]["receipt"] =
			serde_json::json!({"delivered_turn_id":"turn"});

		assert!(crate::composer_send_answered(&before, &after, "Reply UI_READY"));
	}

	#[test]
	fn capture_send_rejects_old_or_unrelated_answers() {
		let mut after = evidence();

		after["feedback"] = serde_json::json!("Accepted by service");

		assert!(!crate::composer_send_answered(&after, &after, "Reply UI_READY"));

		let mut before = evidence();

		before["history"][1]["entries"].as_array_mut().unwrap().remove(0);

		assert!(!crate::composer_send_answered(&before, &after, "Reply UI_READY"));

		let before =
			serde_json::json!({"history":["manager", {"outcome":"available", "entries":[]}]});

		after["history"][1]["entries"][1]["turn_id"] = serde_json::json!("other-turn");

		assert!(!crate::composer_send_answered(&before, &after, "Reply UI_READY"));
	}

	#[test]
	fn capture_send_requires_settled_delivery_and_same_owner() {
		let before =
			serde_json::json!({"history":["manager", {"outcome":"available", "entries":[]}]});

		for (field, value) in [
			("uncertain", serde_json::json!(true)),
			("sending", serde_json::json!(true)),
			("draft", serde_json::json!("retained")),
		] {
			let mut after = evidence();

			after[field] = value;

			assert!(!crate::composer_send_answered(&before, &after, "Reply UI_READY"));
		}

		assert!(!crate::composer_send_answered(&Value::Null, &evidence(), "Reply UI_READY"));

		let mut after = evidence();

		after["history"][1]["entries"][0]["turn_id"] = Value::Null;

		assert!(!crate::composer_send_answered(&before, &after, "Reply UI_READY"));

		after = evidence();
		after["history"][0] = serde_json::json!("other-manager");

		assert!(!crate::composer_send_answered(&before, &after, "Reply UI_READY"));
	}
}
