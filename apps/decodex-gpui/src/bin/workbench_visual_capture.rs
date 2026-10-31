//! Deterministic native screenshot capture for the Codex Workbench design review.
#![cfg(not(test))]

#[allow(dead_code)]
#[path = "../account_login.rs"]
mod account_login;
#[allow(dead_code)]
#[path = "../account_profile.rs"]
mod account_profile;
#[allow(dead_code)]
#[path = "../accounts.rs"]
mod accounts;
#[path = "workbench_visual_capture/send.rs"] mod capture_send;
#[allow(dead_code)]
#[path = "../client_cache.rs"]
mod client_cache;
#[allow(dead_code)]
#[path = "../client_lifecycle.rs"]
mod client_lifecycle;
#[allow(dead_code)]
#[path = "../component_plugins.rs"]
mod component_plugins;
#[allow(dead_code)]
#[path = "../component_research.rs"]
mod component_research;
#[allow(dead_code)]
#[path = "../component_runtime.rs"]
mod component_runtime;
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
#[path = "../frame_trace.rs"]
mod frame_trace;
#[allow(dead_code)]
#[path = "../health_query.rs"]
mod health_query;
#[allow(dead_code)]
#[path = "../history_pager.rs"]
mod history_pager;
#[cfg(target_os = "macos")]
#[path = "probes/native_island.rs"]
mod native_island;
#[allow(dead_code)]
#[path = "../native_menu_bar.rs"]
mod native_menu_bar;
#[cfg(target_os = "macos")]
#[path = "probes/native_scroll.rs"]
mod native_scroll;
#[allow(dead_code)]
#[path = "../native_updater.rs"]
mod native_updater;
#[path = "../panel_preferences.rs"] mod panel_preferences;
#[path = "../session_settings.rs"] mod session_settings;
#[allow(dead_code)]
#[path = "../settings_surface.rs"]
mod settings_surface;
#[allow(dead_code)]
#[path = "../shell.rs"]
mod shell;
#[path = "../ui_feedback.rs"] mod ui_feedback;
#[path = "../ui_frame.rs"] mod ui_frame;
#[path = "../ui_layout.rs"] mod ui_layout;
#[path = "../ui_live_activity.rs"] mod ui_live_activity;
#[allow(dead_code)]
#[path = "../ui_loading.rs"]
mod ui_loading;
#[path = "../ui_motion.rs"] mod ui_motion;
#[path = "../ui_performance.rs"] mod ui_performance;
#[allow(dead_code)]
#[path = "../ui_preferences.rs"]
mod ui_preferences;
#[allow(dead_code)]
#[path = "../ui_scroll.rs"]
mod ui_scroll;
#[allow(dead_code)]
#[path = "../ui_theme.rs"]
mod ui_theme;
#[path = "../ui_work_metrics.rs"] mod ui_work_metrics;
#[path = "../ui_working.rs"] mod ui_working;

use std::{
	env, fs,
	io::Error,
	path::{Path, PathBuf},
	thread,
	time::{Duration, Instant},
};

use gpui::{
	self, AnyWindowHandle, AppContext as _, Context, Entity, IntoElement, Pixels, Render, Result,
	ScrollDelta, ScrollEvent, Size, VisualTestAppContext, Window, WindowHandle,
};
#[allow(dead_code)]
#[cfg(target_os = "macos")]
use objc2 as _;
#[cfg(target_os = "macos")] use objc2_app_kit::{NSResponder, NSView};
#[cfg(target_os = "macos")] use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tokio::runtime::Builder;
#[cfg(target_os = "macos")] use {objc2_app_kit as _, objc2_foundation as _};

use crate::shell::{Destination, Shell, agent_surface::AgentSurface};
use decodex_protocol::{
	AgentClient, AgentMediaRequest, AgentSnapshotResult, AgentSteerIdentity, ClientFailure,
	EntityId,
};
use shell::SettingsWindow;
use ui_theme::{ColorTheme, MOTION_PANEL, appearance::Appearance};

type ServiceProjection = (
	AgentSnapshotResult,
	Option<String>,
	Option<decodex_protocol::AgentHistoryResult>,
	Option<decodex_protocol::AgentRequestResult>,
	Option<decodex_protocol::AgentGuardianReviewsResult>,
	decodex_protocol::ClientProfile,
);

struct CollaborationComponent(Entity<AgentSurface>, bool);
impl Render for CollaborationComponent {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |surface, cx| surface.render_collaboration_component(self.1, cx))
	}
}

fn main() -> Result<()> {
	#[cfg(target_os = "macos")]
	if env::var_os("DECODEX_VISUAL_NATIVE_ISLAND").is_some() {
		crate::native_island::run();

		return Ok(());
	}
	#[cfg(target_os = "macos")]
	if env::var_os("DECODEX_VISUAL_NATIVE_SCROLL").is_some() {
		native_scroll::run();

		return Ok(());
	}

	let (output, mut cx) = capture_context()?;
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
	let window: AnyWindowHandle = if env::var("DECODEX_VISUAL_COMPONENT").as_deref()
		== Ok("collaboration")
	{
		capture_collaboration(&mut cx)?
	} else if integrations {
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
		capture_shell(&mut cx, destination, left_sidebar_visible, inspector_visible)?
	};

	settle_capture_layout(&mut cx, window)?;
	capture_graph_zoom(&mut cx, window)?;
	capture_component_motion(&mut cx, window, &output)?;

	if let Some(panel_motion) = panel_motion {
		animate_panel_motion(&mut cx, window, &panel_motion)?;
	}

	if integrations {
		capture_integrations(&mut cx, window, &output)?;
	}

	capture_interactions(&mut cx, window)?;
	capture_frame_cost(&mut cx, window, &output)?;

	let screenshot = cx.capture_screenshot(window)?;

	screenshot.save(&output)?;
	if let Some(composer_output) = env::var_os("DECODEX_VISUAL_COMPOSER_OUTPUT") {
		let children: Vec<_> = cx
			.update(|cx| cx.windows())
			.into_iter()
			.filter(|child| {
				cx.update_window(*child, |_, window, _| window.window_title() == "Decodex Composer")
					.unwrap_or(false)
			})
			.collect();
		let [composer] = children.as_slice() else {
			return Err(Error::other("Composer capture requires one native child window").into());
		};
		cx.capture_screenshot(*composer)?.save(PathBuf::from(composer_output))?;
	}

	println!("{}", output.display());

	Ok(())
}

fn capture_collaboration(cx: &mut VisualTestAppContext) -> Result<AnyWindowHandle> {
	let speech = env::var("DECODEX_VISUAL_COMPONENT_MOTION").as_deref() == Ok("speech");
	let size = if speech { (520., 420.) } else { (1_536., 1_024.) };

	Ok(cx
		.open_offscreen_window(gpui::size(gpui::px(size.0), gpui::px(size.1)), |_, cx| {
			cx.new(|cx| {
				let surface = cx.new(|cx| {
					let mut surface = AgentSurface::new(cx);

					surface.visual_workspace_fixture(cx);
					surface.visual_workspace_page("workbench", cx);

					if env::var_os("DECODEX_VISUAL_PROJECT_SCOPES").is_some() {
						surface.visual_project_scopes();
					}

					if let Ok(count) = env::var("DECODEX_VISUAL_GRAPH_AGENTS") {
						surface.visual_relation_density(count.parse().expect("agent count"), cx);
					}
					if let Some(path) = env::var_os("DECODEX_VISUAL_REVIEW_CAPTURE") {
						surface.visual_review_capture(Path::new(&path), cx);
					}

					surface
				});

				cx.observe(&surface, |_, _, cx| cx.notify()).detach();

				CollaborationComponent(surface, !speech)
			})
		})?
		.into())
}

fn capture_frame_cost(
	cx: &mut VisualTestAppContext,
	window: AnyWindowHandle,
	output: &Path,
) -> Result<()> {
	if env::var_os("DECODEX_VISUAL_FRAME_COST").is_none() {
		return Ok(());
	}

	let mut samples = Vec::with_capacity(120);

	for _ in 0..120 {
		let start = Instant::now();

		cx.update_window(window, |_, window, cx| {
			window.refresh();
			window.draw(cx).clear(cx);
		})?;
		samples.push(start.elapsed().as_secs_f64() * 1_000.);
	}

	let mean = samples.iter().sum::<f64>() / samples.len() as f64;

	samples.sort_by(f64::total_cmp);

	fs::write(
		output.with_extension("frames.json"),
		serde_json::to_vec_pretty(&serde_json::json!({
			"scope": "CPU frame construction in a settled native offscreen window; excludes GPU and display timing",
			"samples": samples.len(), "mean_ms": mean,
			"p50_ms": samples[samples.len() / 2],
			"p95_ms": samples[samples.len() * 95 / 100],
			"max_ms": samples[samples.len() - 1]
		}))?,
	)?;

	Ok(())
}

fn capture_component_motion(
	cx: &mut VisualTestAppContext,
	window: AnyWindowHandle,
	output: &Path,
) -> Result<()> {
	match env::var("DECODEX_VISUAL_COMPONENT_MOTION").as_deref() {
		Ok("actions") => capture_collaboration_actions(cx, window, output),
		Ok("speech") => capture_speech_sequence(cx, window, output),
		_ => Ok(()),
	}
}

fn capture_graph_zoom(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Result<()> {
	let Ok(scale) = env::var("DECODEX_VISUAL_GRAPH_ZOOM") else { return Ok(()) };
	let scale: f32 = scale.parse()?;
	let surface = cx.update_window(window, |view, _, cx| {
		view.downcast::<CollaborationComponent>()
			.expect("graph zoom requires the isolated collaboration component")
			.read(cx)
			.0
			.clone()
	})?;

	surface.update(cx, |surface, cx| {
		if let Ok(view) = env::var("DECODEX_VISUAL_GRAPH_VIEW") {
			assert!(["links", "together"].contains(&view.as_str()), "known agent view");

			surface.visual_relation_presentation(view == "together", cx);
		}
		if let Ok(layout) = env::var("DECODEX_VISUAL_GRAPH_LAYOUT") {
			surface.visual_relation_layout(&layout, cx);
		}

		surface.visual_relation_zoom(scale, cx);

		if let Ok(key) = env::var("DECODEX_VISUAL_GRAPH_TEAM") {
			surface.visual_relation_team(&key, cx);
		}
	});

	settle_capture_layout(cx, window)
}

fn capture_collaboration_actions(
	cx: &mut VisualTestAppContext,
	window: AnyWindowHandle,
	output: &Path,
) -> Result<()> {
	let surface = cx.update_window(window, |view, _, cx| {
		view.downcast::<CollaborationComponent>()
			.expect("isolated component capture")
			.read(cx)
			.0
			.clone()
	})?;

	for (step, delay, name) in
		[(3, 300, "three"), (4, 80, "moving"), (4, 300, "settled"), (4, 6_500, "one")]
	{
		surface.update(cx, |surface, cx| surface.visual_collaboration_actions(step, cx));
		cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;

		thread::sleep(Duration::from_millis(delay));

		cx.advance_clock(Duration::from_millis(delay));
		cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;
		cx.capture_screenshot(window)?.save(output.with_extension(format!("{name}.png")))?;
	}

	Ok(())
}

fn capture_speech_sequence(
	cx: &mut VisualTestAppContext,
	window: AnyWindowHandle,
	output: &Path,
) -> Result<()> {
	let surface = cx.update_window(window, |view, _, cx| {
		view.downcast::<CollaborationComponent>()
			.expect("isolated speech capture")
			.read(cx)
			.0
			.clone()
	})?;

	surface.update(cx, |surface, cx| surface.visual_relation_agent("agent", cx));

	let frames = output.with_extension("frames");

	fs::create_dir_all(&frames)?;

	let start = Instant::now();
	let mut samples = Vec::new();

	for frame in 0..75 {
		if frame % 15 == 0 {
			surface.update(cx, |surface, cx| surface.visual_speech_sample(frame / 15, cx));
		}

		cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;

		// Uncompressed capture keeps PNG encoding out of the animation clock.
		let file = frames.join(format!("{frame:04}.bmp"));

		cx.capture_screenshot(window)?.save(&file)?;
		samples.push(serde_json::json!({"frame":frame,"elapsed":start.elapsed().as_secs_f64()}));

		thread::sleep(Duration::from_millis(16));

		cx.advance_clock(Duration::from_millis(16));
	}

	fs::write(output.with_extension("frames.json"), serde_json::to_vec_pretty(&samples)?)?;

	Ok(())
}

fn settle_capture_layout(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Result<()> {
	cx.run_until_parked();
	cx.update_window(window, |_, window, _| window.refresh())?;
	cx.run_until_parked();
	// GPUI element animations use the monotonic wall clock, while async timers
	// use the visual-test dispatcher clock. Render once to start the element
	// animation, then wait on the same clock that drives it.
	cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;

	thread::sleep(MOTION_PANEL + Duration::from_millis(40));

	cx.advance_clock(Duration::from_millis(16));
	cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;
	cx.run_until_parked();

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
		ScrollEvent {
			position: gpui::point(gpui::px(450.), gpui::px(620.)),
			delta: ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-460.))),
			..Default::default()
		},
	);
	cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;

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
		cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;

		thread::sleep(MOTION_PANEL / 2);

		cx.advance_clock(Duration::from_millis(16));
		cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;
		cx.run_until_parked();
	}

	Ok(())
}

// Sample dismissal over the real composer, including its Live button.
fn capture_status_dismissal(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Result<()> {
	let Some(delay) = env::var("DECODEX_VISUAL_POPOVER_CLOSE_MS")
		.or_else(|_| env::var("DECODEX_VISUAL_STATUS_CLOSE_MS"))
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
	else {
		return Ok(());
	};

	cx.simulate_click(window, gpui::point(gpui::px(1_200.), gpui::px(815.)), Default::default());
	cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;

	thread::sleep(Duration::from_millis(delay));

	cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;

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

	if let Ok(distance) = env::var("DECODEX_VISUAL_SCROLL_Y") {
		let distance: f32 = distance.parse()?;

		cx.simulate_event(
			window,
			ScrollEvent {
				position: gpui::point(gpui::px(800.), gpui::px(420.)),
				delta: ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-distance))),
				..Default::default()
			},
		);
		cx.advance_clock(Duration::from_secs(1));
		cx.run_until_parked();

		thread::sleep(MOTION_PANEL + Duration::from_millis(80));

		cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;
	}

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
	cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;

	thread::sleep(MOTION_PANEL + Duration::from_millis(40));

	cx.update_window(window, |_, window, cx| {
		window.refresh();
		window.draw(cx).clear(cx);
	})?;

	Ok(())
}

fn capture_context() -> Result<(PathBuf, VisualTestAppContext)> {
	let output = env::var_os("DECODEX_VISUAL_OUTPUT")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from("target/visual-tests/codex-workbench.png"));

	if let Some(parent) = output.parent() {
		fs::create_dir_all(parent)?;
	}

	let appearance = match env::var("DECODEX_VISUAL_APPEARANCE").as_deref() {
		Ok("system") => Appearance::System,
		Ok("light") => Appearance::Light,
		Ok("dark") | Err(_) => Appearance::Dark,
		Ok(value) => return Err(Error::other(format!("Unknown appearance: {value}")).into()),
	};
	let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));

	cx.update(|cx| appearance.apply(cx));

	if let Ok(id) = env::var("DECODEX_VISUAL_THEME") {
		let theme = ColorTheme::from_id(&id)
			.ok_or_else(|| Error::other(format!("Unknown color theme: {id}")))?;

		cx.update(|cx| theme.apply(cx));
	}

	cx.update(shell::bind_keys);

	Ok((output, cx))
}

fn capture_shell(
	cx: &mut VisualTestAppContext,
	destination: Destination,
	left_sidebar_visible: bool,
	inspector_visible: bool,
) -> Result<AnyWindowHandle> {
	let size = capture_size()?;

	Ok(if env::var_os("DECODEX_VISUAL_STANDALONE_SETTINGS").is_some() {
		cx.open_offscreen_window(size, |window, cx| {
			let owner =
				cx.new(|cx| Shell::visual_destination(destination, true, false, window, cx));

			cx.new(|cx| SettingsWindow::new(owner, window, cx))
		})?
		.into()
	} else {
		cx.open_offscreen_window(size, |window, cx| {
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
	})
}

fn capture_size() -> Result<Size<Pixels>> {
	Ok(if let Ok(size) = env::var("DECODEX_VISUAL_SIZE") {
		let (width, height) = size
			.split_once('x')
			.ok_or_else(|| Error::other("Use WIDTHxHEIGHT for DECODEX_VISUAL_SIZE"))?;

		gpui::size(gpui::px(width.parse()?), gpui::px(height.parse()?))
	} else {
		gpui::size(gpui::px(1_248.), gpui::px(840.))
	})
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

		window.draw(cx).clear(cx);

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
		if capture_send::composer_send_answered(&before, &evidence, message) {
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

	cx.update_window(handle.into(), |_, window, cx| window.draw(cx).clear(cx))?;
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
			window.draw(cx).clear(cx);

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

		window.draw(cx).clear(cx);

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
			window.draw(cx).clear(cx);

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
