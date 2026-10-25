//! Display-backed acceptance fixture. Uses production rendering, scrolling and native material.
use std::{
	env,
	time::{Duration, Instant},
};

use gpui::{
	ActivationPolicy, AnyWindowHandle, App, AppContext as _, Bounds, PlatformInput, ScrollDelta,
	ScrollEvent, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions,
};

use crate::{
	frame_trace,
	shell::{self, Destination, Shell},
	ui_frame, ui_motion,
	ui_theme::window_material,
};

pub(super) fn run() {
	gpui_platform::application().run(|cx| {
		cx.set_activation_policy(ActivationPolicy::Regular);

		frame_trace::start(cx);
		shell::bind_keys(cx);

		let bounds = Bounds::centered(None, gpui::size(gpui::px(1_248.), gpui::px(840.)), cx);
		let window = cx
			.open_window(
				WindowOptions {
					titlebar: Some(TitlebarOptions {
						title: Some("Mermaid Scroll Review".into()),
						appears_transparent: true,
						..Default::default()
					}),
					window_bounds: Some(WindowBounds::Windowed(bounds)),
					window_background: WindowBackgroundAppearance::Blurred,
					..ui_motion::window_options()
				},
				|window, cx| {
					cx.new(|cx| {
						ui_motion::observe_activation(window, cx);

						Shell::visual_destination(Destination::Agent, false, false, window, cx)
					})
				},
			)
			.expect("open display-backed scroll fixture");

		window
			.update(cx, |_, window, cx| {
				window_material::configure(window);

				eprintln!(
					"native-scroll: display interval {:?}, scale {}, viewport {:?}",
					window.refresh_interval(cx),
					window.scale_factor(),
					window.viewport_size()
				);

				if env::var("DECODEX_VISUAL_NATIVE_SCROLL").as_deref() == Ok("auto") {
					// A timer can recover from background frame parking. A display callback cannot.
					keep_foreground(window.window_handle(), cx);

					let start = Instant::now();

					window.on_next_frame(move |window, cx| scroll_frame(start, start, window, cx));
				}
			})
			.expect("configure scroll fixture");

		cx.activate(true);
	});
}

fn keep_foreground(window: AnyWindowHandle, cx: &mut App) {
	cx.spawn(async move |cx| {
		let started = Instant::now();

		while started.elapsed() < Duration::from_secs(55) {
			cx.background_executor().timer(Duration::from_secs(1)).await;

			cx.update(|cx| {
				if !ui_frame::application_active() {
					cx.activate(true);

					let _ = window.update(cx, |_, window, _| window.activate_window());
				}
			});
		}
	})
	.detach();
}

// Drive the production wheel handler from the real display callback. Unlike an offscreen draw
// loop, this includes native frame scheduling, Metal submission and WindowServer composition.
fn scroll_frame(start: Instant, previous: Instant, window: &mut Window, cx: &mut App) {
	let now = Instant::now();
	let elapsed = now.duration_since(start).as_secs_f32();

	if elapsed >= 55. {
		eprintln!("native-scroll: display-paced sequence complete");

		return;
	}
	if elapsed >= 3. {
		let phase = ((elapsed - 3.) / 6.) as usize;
		let speed = if phase < 2 { 600. } else { 1_800. };
		let direction = if phase.is_multiple_of(2) { -1. } else { 1. };

		window.dispatch_event(
			PlatformInput::Scroll(ScrollEvent {
				position: window.viewport_size().map(|value| value / 2.).into(),
				delta: ScrollDelta::Pixels(gpui::point(
					gpui::px(0.),
					gpui::px(direction * speed * now.duration_since(previous).as_secs_f32()),
				)),
				..Default::default()
			}),
			cx,
		);
	}

	window.on_next_frame(move |window, cx| scroll_frame(start, now, window, cx));
}
