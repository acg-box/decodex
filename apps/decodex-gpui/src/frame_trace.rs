//! Opt-in, bounded GPUI frame measurements. No conversation content is recorded.
//! Set DECODEX_FRAME_TRACE to an output path before launching a release build.
use std::{
	env, fs, thread,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use gpui::profiler::{self, FrameEvent, FrameTimingCollector};

pub(crate) fn start() {
	let Some(path) = env::var_os("DECODEX_FRAME_TRACE") else { return };
	let start = Instant::now();
	let started_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis());

	profiler::set_trace_enabled(true);

	let mut collector = FrameTimingCollector::new();

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
		let report = serde_json::json!({"schema": 2, "started_at_ms": started_at_ms, "note": "CPU draw and platform submission timing; excludes GPU completion and screen scanout. Animation intervals exclude idle gaps.", "frames": rows, "presentations": presentations});

		if let Err(error) = fs::write(path, report.to_string()) {
			eprintln!("Could not write frame measurements: {error}");
		}
	});
}
