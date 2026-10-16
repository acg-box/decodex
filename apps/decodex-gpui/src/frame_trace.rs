//! Opt-in, bounded GPUI frame measurements. No conversation content is recorded.
//! Set DECODEX_FRAME_TRACE to an output path before launching a release build.
use std::{
	env, fs,
	sync::{Arc, Mutex, OnceLock},
	thread,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use gpui::profiler::{self, FrameEvent, FrameTimingCollector};

static ROWS: OnceLock<(Instant, Mutex<Vec<serde_json::Value>>)> = OnceLock::new();

/// Trace row presentation alongside frame timing, without message content.
pub(crate) fn row_arrival(position: u64, phase: &str, progress: f32, height: f32) {
	record_row(position, phase, Some(progress), height, None);
}

pub(crate) fn row_layout(position: u64, height: f32, scroll_offset: f32) {
	record_row(position, "layout", None, height, Some(scroll_offset));
}

fn record_row(
	position: u64,
	phase: &str,
	progress: Option<f32>,
	height: f32,
	scroll_offset: Option<f32>,
) {
	if !profiler::trace_enabled() {
		return;
	}
	let Some((start, rows)) = ROWS.get() else { return };
	let mut rows = rows.lock().expect("row frame measurements");
	if rows.len() < 8_192 {
		rows.push(serde_json::json!({"at_ms": start.elapsed().as_secs_f64() * 1_000., "position": position, "phase": phase, "progress": progress, "height": height, "scroll_offset": scroll_offset}));
	}
}

pub(crate) fn start(cx: &mut gpui::App) {
	let Some(path) = env::var_os("DECODEX_FRAME_TRACE") else { return };
	let start = Instant::now();
	let _ = ROWS.set((start, Mutex::new(Vec::new())));
	let started_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis());

	profiler::set_trace_enabled(true);

	let mut collector = FrameTimingCollector::new();
	let activity = Arc::new(Mutex::new(Vec::new()));
	let observations = activity.clone();
	cx.spawn(async move |cx| {
		while start.elapsed() < Duration::from_secs(60) {
			let sample = cx.update(|cx| {
				let windows: Vec<_> = cx.windows().into_iter().filter_map(|handle| {
					handle.update(cx, |_, window, _| {
						#[cfg(all(target_os = "macos", not(test)))]
						let glass = crate::ui_theme::native_glass_panel::observed_style(window);
						#[cfg(any(not(target_os = "macos"), test))]
						let glass = None::<isize>;
						serde_json::json!({"window": format!("{:?}", handle.window_id()), "key": window.is_window_active(), "glass_style": glass})
					}).ok()
				}).collect();
				serde_json::json!({"at_ms": start.elapsed().as_secs_f64() * 1_000., "application_active": crate::ui_frame::application_active(), "windows": windows})
			});
			observations.lock().expect("frame activity").push(sample);
			cx.background_executor().timer(Duration::from_millis(100)).await;
		}
	}).detach();

	thread::spawn(move || {
		// Collect for one minute, then stop completely. The GPUI ring is bounded.
		thread::sleep(Duration::from_secs(60));

		let frames = collector.collect_unseen();

		profiler::set_trace_enabled(false);

		let presentations: Vec<_> = frames
			.iter()
			.filter_map(|event| {
				let FrameEvent::Present(f) = event else { return None };
				Some(serde_json::json!({
					"window": format!("{:?}", f.window_id),
					"start_ms": f.present_start.duration_since(start).as_secs_f64() * 1_000.,
					"submit_ms": f.present_duration().as_secs_f64() * 1_000.,
					"animation_interval_ms": f.animation_interval.map(|d| d.as_secs_f64() * 1_000.),
				}))
			})
			.collect();
		let rows: Vec<_> = frames
			.iter()
			.filter_map(|event| {
				let FrameEvent::Draw(f) = event else { return None };

				Some(serde_json::json!({
					"window": format!("{:?}", f.window_id),
					"start_ms": f.draw_start.duration_since(start).as_secs_f64() * 1_000.,
					"draw_ms": f.draw_duration().as_secs_f64() * 1_000.,
					"dirty_to_draw_ms": f.dirty_to_draw_duration().map(|d| d.as_secs_f64() * 1_000.),
					"invalidations": f.invalidations,
				}))
			})
			.collect();
		let arrivals =
			ROWS.get().map(|(_, rows)| rows.lock().expect("row frame measurements").clone());
		let report = serde_json::json!({"schema": 4, "started_at_ms": started_at_ms, "note": "CPU draw and platform submission timing; excludes GPU completion and screen scanout. Activity samples distinguish foreground from hidden or inactive windows. Native glass styles: 0 regular, 1 clear. Row arrivals record presentation progress without message content.", "frames": rows, "presentations": presentations, "activity": *activity.lock().expect("frame activity"), "row_arrivals": arrivals});

		if let Err(error) = fs::write(path, report.to_string()) {
			eprintln!("Could not write frame measurements: {error}");
		}
	});
}
