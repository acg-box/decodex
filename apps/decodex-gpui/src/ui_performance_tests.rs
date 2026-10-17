//! Check rolling metrics and keep Debug independent of full task tracing.
use super::*;

#[gpui::test]
fn hover_expands_and_leaving_closes_without_a_tooltip(cx: &mut gpui::TestAppContext) {
	preference(Some(true));
	let (island, visual) = cx.add_window_view(|_, cx| PerformanceIsland::new(cx));
	visual.update(|window, cx| window.draw(cx).clear(cx));
	let toggle = visual.debug_bounds("performance-island-toggle").expect("island control");
	visual.simulate_mouse_move(toggle.center(), None, Default::default());
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(island.read_with(visual, |s, _| s.expansion.is_open()));
	visual.simulate_mouse_move(
		gpui::point(gpui::px(600.), gpui::px(400.)),
		None,
		Default::default(),
	);
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(!island.read_with(visual, |s, _| s.expansion.is_open()));
	visual.simulate_mouse_move(toggle.center(), None, Default::default());
	visual.simulate_click(toggle.center(), Default::default());
	visual.simulate_mouse_move(
		gpui::point(gpui::px(600.), gpui::px(400.)),
		None,
		Default::default(),
	);
	assert_eq!(island.read_with(visual, |s, _| s.expansion), Expansion::Clicked);
	visual.simulate_click(toggle.center(), Default::default());
	assert!(!island.read_with(visual, |s, _| s.expansion.is_open()));
	preference(Some(false));
}

#[gpui::test]
fn expanded_details_and_chart_fit_inside_the_compact_capsule(cx: &mut gpui::TestAppContext) {
	preference(Some(true));
	let (_, visual) = cx.add_window_view(|_, cx| {
		let mut island = PerformanceIsland::new(cx);
		island.expansion = Expansion::Clicked;
		island
	});
	visual.update(|window, cx| window.draw(cx).clear(cx));
	let details = visual.debug_bounds("performance-island-details").expect("metric grid");
	let chart = visual.debug_bounds("performance-island-chart").expect("draw-time chart");
	let capsule = visual.debug_bounds("performance-island").unwrap();
	assert!(details.bottom() <= capsule.bottom(), "{details:?} outside {capsule:?}");
	assert!(details.right() <= capsule.right());
	assert!(chart.size.height >= gpui::px(28.));
	assert_eq!(capsule.size, gpui::size(gpui::px(304.), gpui::px(116.)));
	assert!(visual.debug_bounds("performance-island-note").is_none());
	preference(Some(false));
}

#[gpui::test]
fn draw_history_keeps_real_samples_in_background_without_inserting_missing_zeros(
	cx: &mut gpui::TestAppContext,
) {
	let island = cx.new(PerformanceIsland::new);
	island.update(cx, |island, _| {
		for index in 1..=40 {
			island.record(Metrics { draw_frames: 5, draw_ms: index as f64, ..Default::default() });
			island.record(Metrics::default());
		}
		assert_eq!(island.history.len(), 32);
		assert_eq!(island.history.front(), Some(&9.));
		assert_eq!(island.history.back(), Some(&40.));
		assert!(island.history.iter().all(|value| *value > 0.));
	});
}

#[gpui::test]
fn metrics_use_only_the_new_histogram_samples(cx: &mut gpui::TestAppContext) {
	let (_, visual) = cx.add_window_view(|_, cx| PerformanceIsland::new(cx));
	visual.update(|window, _| {
		let mut previous = window.frame_duration_snapshot();
		previous.draw_duration_histogram.record(100_000_000).unwrap();
		previous.present_interval_histogram.record(100_000_000).unwrap();
		let mut next = previous.clone();
		for value in [2_000_000, 4_000_000] {
			next.draw_duration_histogram.record(value).unwrap();
		}
		for value in [6_000_000, 12_000_000] {
			next.present_interval_histogram.record(value).unwrap();
		}
		next.dirty_to_present_histogram.record(9_000_000).unwrap();
		let metrics = Metrics::collect(next, &previous, 0.5).unwrap();
		assert_eq!(metrics.draw_frames, 2);
		assert_eq!(metrics.fps, 4.);
		assert!((4. ..4.01).contains(&metrics.draw_ms));
		assert!((9. ..9.01).contains(&metrics.submit_ms));
	});
}

#[gpui::test]
fn disabling_the_overlay_stops_sampling_without_enabling_task_traces(
	cx: &mut gpui::TestAppContext,
) {
	let was_tracing = gpui::profiler::trace_enabled();
	preference(Some(true));
	let (island, visual) = cx.add_window_view(|_, cx| PerformanceIsland::new(cx));
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(island.read_with(visual, |s, _| s.task.is_some()));
	assert_eq!(gpui::profiler::trace_enabled(), was_tracing);
	preference(Some(false));
	visual.update(|window, cx| {
		cx.refresh_windows();
		window.draw(cx).clear(cx);
	});
	assert!(island.read_with(visual, |s, _| s.task.is_none()));
	assert_eq!(gpui::profiler::trace_enabled(), was_tracing);
}
