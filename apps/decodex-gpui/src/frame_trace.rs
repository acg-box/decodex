//! Opt-in, bounded GPUI frame measurements. No conversation content is recorded.
//! Set DECODEX_FRAME_TRACE to an output path before launching a release build.
use std::time::{Duration, Instant};

pub(crate) fn start() {
	let Some(path) = std::env::var_os("DECODEX_FRAME_TRACE") else { return };
	let start = Instant::now();

	gpui::profiler::set_frame_trace_enabled(true);

	let mut collector = gpui::profiler::FrameTimingCollector::new();

	std::thread::spawn(move || {
		// Collect for one minute, then stop completely. The GPUI ring is bounded.
		std::thread::sleep(Duration::from_secs(60));

		let frames = collector.collect_unseen();

		gpui::profiler::set_frame_trace_enabled(false);

		let rows: Vec<_> = frames
			.into_iter()
			.map(|f| {
				serde_json::json!({
					"window": format!("{:?}", f.window_id),
					"start_ms": f.draw_start.duration_since(start).as_secs_f64() * 1_000.,
					"draw_ms": f.draw_duration().as_secs_f64() * 1_000.,
					"dirty_to_draw_ms": f.dirty_to_draw_duration().map(|d| d.as_secs_f64() * 1_000.),
					"invalidations": f.invalidations,
				})
			})
			.collect();
		let report = serde_json::json!({"schema": 1, "note": "CPU draw timing; excludes GPU presentation. Idle gaps are not dropped frames.", "frames": rows});

		if let Err(error) = std::fs::write(path, report.to_string()) {
			eprintln!("Could not write frame measurements: {error}");
		}
	});
}
