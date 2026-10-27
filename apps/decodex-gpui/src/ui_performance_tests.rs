//! Check rolling metrics and keep Debug independent of full task tracing.
use std::{thread, time::Duration};

use gpui::{AppContext as _, TestAppContext, profiler};

use crate::{
	ui_performance::{self, Activity, Expansion, Metrics, PerformanceIsland},
	ui_work_metrics::{Live, Sample, Summary},
};
use decodex_protocol::{AgentOutputRate, AgentThreadMetrics, AgentThreadTokenBreakdown};

#[gpui::test]
fn work_capsule_grows_and_shrinks_with_content_instead_of_the_agent_count(cx: &mut TestAppContext) {
	ui_performance::preference(Some(true));

	let idle = Summary { samples: vec![Default::default(); 96], ..Default::default() };
	let (island, visual) = cx.add_window_view(|_, cx| {
		let mut island = PerformanceIsland::new(cx);

		island.work = Some(idle.clone());

		island
	});

	visual.update(crate::ui_frame::draw_frame);

	let before = visual.debug_bounds("activity-work-island").unwrap();
	let label = visual.debug_bounds("activity-live-rate").unwrap();
	let debug = visual.debug_bounds("performance-island").unwrap();
	let fps = visual.debug_bounds("performance-fps").unwrap();

	assert!((label.center().x - before.center().x).abs() < gpui::px(0.5));
	assert!(
		(label.center().y - before.center().y).abs() < gpui::px(0.5),
		"work label {label:?} is not centered in {before:?}"
	);
	assert!(
		(fps.center().y - debug.center().y).abs() < gpui::px(0.5),
		"fps label {fps:?} is not centered in {debug:?}"
	);

	island.update(visual, |s, cx| {
		s.set_work(
			Some(Summary {
				working: 1,
				live: vec![],
				samples: vec![crate::ui_work_metrics::Sample {
					stats: Some(AgentThreadMetrics {
						generating: true,
						rate: Some(AgentOutputRate {
							estimated: true,
							active: true,
							output_tokens: 12_345,
							duration_ms: 1_000,
							started_at_ms: 0,
							completed_at_ms: Some(1_000),
						}),
						..Default::default()
					}),
					..Default::default()
				}],
			}),
			cx,
		);
	});
	visual.update(crate::ui_frame::draw_frame);

	thread::sleep(Duration::from_millis(250));

	visual.update(crate::ui_frame::draw_frame);

	let busy = visual.debug_bounds("activity-work-island").unwrap();

	assert_eq!(
		busy.size.width,
		visual.debug_bounds("performance-island").unwrap().size.width,
		"resting capsules share one footprint even when a value grows"
	);

	let label = visual.debug_bounds("activity-live-rate").unwrap();

	assert!(busy.size.width > before.size.width);
	assert!(label.left() > busy.left() && label.right() < busy.right());

	island.update(visual, |s, cx| s.set_work(Some(idle), cx));
	visual.update(crate::ui_frame::draw_frame);

	thread::sleep(Duration::from_millis(250));

	visual.update(crate::ui_frame::draw_frame);

	assert_eq!(visual.debug_bounds("activity-work-island").unwrap().size.width, before.size.width);

	ui_performance::preference(Some(false));
}

#[gpui::test]
fn work_statistics_fit_with_debug_and_with_more_than_ninety_agents(cx: &mut TestAppContext) {
	for debug in [false, true] {
		ui_performance::preference(Some(debug));

		let (island, visual) = cx.add_window_view(|_, cx| {
			let mut island = PerformanceIsland::new(cx);

			island.work = Some(Summary {
				working: 92,
				samples: vec![
					Sample {
						tokens: Some(13_000_000_000),
						breakdown: Some(AgentThreadTokenBreakdown {
							input_tokens: 12_000_000_000,
							cached_input_tokens: 8_000_000_000,
							output_tokens: 1_000_000_000,
							reasoning_output_tokens: 600_000_000,
						}),
						..Default::default()
					};
					92
				],
				live: (1..=92)
					.map(|index| Live {
						key: format!("live-{index}"),
						agent: format!("A{index}"),
						action: "cargo test".into(),
						sample: Sample {
							stats: Some(AgentThreadMetrics {
								active_ms: Some(8_000),
								..Default::default()
							}),
							..Default::default()
						},
					})
					.collect(),
			});
			island.expansion = Expansion::Clicked;

			island
		});

		visual.update(crate::ui_frame::draw_current_frame);

		let pill = visual.debug_bounds("activity-work-island").unwrap();
		let work = visual.debug_bounds("activity-work-details").unwrap();

		assert!(work.bottom() <= pill.bottom());
		assert!(work.right() <= pill.right());

		let first = visual.debug_bounds("activity-live-live-1").unwrap();

		assert!(first.left() > pill.left() && first.right() < pill.right());
		assert!(
			visual.debug_bounds("activity-live-live-92").is_none(),
			"offscreen work is not rendered"
		);
		assert_eq!(pill.size.height, gpui::px(182.));
		assert!(visual.debug_bounds("activity-work-total").is_some());
		let breakdown = visual.debug_bounds("activity-work-breakdown").unwrap();
		assert!(breakdown.right() <= pill.right() && breakdown.bottom() <= pill.bottom());
		for id in [
			"performance-work-input-row",
			"performance-work-cached-row",
			"performance-work-output-row",
			"performance-work-reasoning-row",
		] {
			let cell = visual.debug_bounds(id).unwrap();

			assert!(cell.left() >= breakdown.left() && cell.right() <= breakdown.right());
		}
		assert!(visual.debug_bounds("performance-needs-you").is_none());
		assert!(visual.debug_bounds("performance-agent-time").is_none());
		assert_eq!(island.read_with(visual, |s, _| s.expansion), Expansion::Clicked);

		if debug {
			assert!(visual.debug_bounds("performance-island-details").is_none());

			let control = visual.debug_bounds("performance-island-toggle").unwrap();

			visual.simulate_click(control.center(), Default::default());
			visual.update(crate::ui_frame::draw_frame);

			thread::sleep(Duration::from_millis(250));

			visual.update(crate::ui_frame::draw_frame);

			let details = visual.debug_bounds("performance-island-details").unwrap();
			let debug_pill = visual.debug_bounds("performance-island").unwrap();

			assert!(details.bottom() <= debug_pill.bottom(), "{details:?} outside {debug_pill:?}");
			assert!(!island.read_with(visual, |s, _| s.expanded(Activity::Work)));
			assert!(visual.debug_bounds("activity-work-details").is_none());
		} else {
			assert!(visual.debug_bounds("performance-island-details").is_none());
			assert!(island.read_with(visual, |s, _| s.task.is_none()));
		}
	}

	ui_performance::preference(Some(false));
}

#[gpui::test]
fn hover_expands_and_leaving_closes_without_a_tooltip(cx: &mut TestAppContext) {
	ui_performance::preference(Some(true));

	let (island, visual) = cx.add_window_view(|_, cx| {
		let mut island = PerformanceIsland::new(cx);

		island.metrics.fps = 165.;
		island.metrics.draw_frames = 1;
		island.host.gpu_percent = Some(100.);

		island
	});

	visual.update(crate::ui_frame::draw_current_frame);

	let toggle = visual.debug_bounds("performance-island-toggle").expect("island control");
	let fps = visual.debug_bounds("performance-fps").unwrap();

	assert!(fps.right() < toggle.right());
	assert_eq!(
		visual.debug_bounds("performance-island").unwrap().size.width,
		gpui::px(crate::ui_live_activity::MIN_CAPSULE_WIDTH)
	);
	assert!(visual.debug_bounds("performance-gpu-label").is_none());

	visual.simulate_mouse_move(toggle.center(), None, Default::default());
	visual.update(crate::ui_frame::draw_current_frame);

	assert!(island.read_with(visual, |s, _| s.expansion.is_open()));

	thread::sleep(Duration::from_millis(250));

	visual.update(crate::ui_frame::draw_frame);

	let expanded = visual.debug_bounds("performance-island-toggle").unwrap();
	let gpu = visual.debug_bounds("performance-gpu-label").unwrap();

	assert!(gpu.left() - visual.debug_bounds("performance-fps").unwrap().right() >= gpui::px(12.));
	assert!(visual.debug_bounds("performance-gpu").unwrap().right() < expanded.right());

	visual.simulate_mouse_move(
		gpui::point(gpui::px(600.), gpui::px(400.)),
		None,
		Default::default(),
	);
	visual.update(crate::ui_frame::draw_current_frame);

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

	ui_performance::preference(Some(false));
}

#[gpui::test]
fn expanded_details_and_chart_fit_inside_the_compact_capsule(cx: &mut TestAppContext) {
	ui_performance::preference(Some(true));

	let (_, visual) = cx.add_window_view(|_, cx| {
		let mut island = PerformanceIsland::new(cx);

		island.expansion = Expansion::Clicked;

		island
	});

	visual.update(crate::ui_frame::draw_current_frame);

	let details = visual.debug_bounds("performance-island-details").expect("metric grid");
	let chart = visual.debug_bounds("performance-island-chart").expect("draw-time chart");
	let capsule = visual.debug_bounds("performance-island").unwrap();

	assert!(details.bottom() <= capsule.bottom(), "{details:?} outside {capsule:?}");
	assert!(details.right() <= capsule.right());
	assert!(chart.size.height >= gpui::px(28.));

	let cells = ["performance-cpu-row", "performance-memory-row", "performance-latency-row"]
		.map(|id| visual.debug_bounds(id).unwrap());
	let draw = visual.debug_bounds("performance-draw-row").unwrap();

	assert_eq!(cells[0].left(), draw.left());
	assert!(
		(cells[2].right() - chart.right()).abs() < gpui::px(0.01),
		"{cells:?} against {chart:?}"
	);

	for pair in cells.windows(2) {
		assert_eq!(pair[0].top(), pair[1].top());
		assert_eq!(pair[1].left() - pair[0].right(), gpui::px(12.));
	}

	assert_eq!(chart.top(), draw.top());
	assert_eq!(chart.left() - draw.right(), gpui::px(8.));
	assert_eq!(visual.debug_bounds("performance-fps-label").unwrap().left(), draw.left());
	assert_eq!(visual.debug_bounds("performance-gpu").unwrap().right(), chart.right());
	assert_eq!(capsule.size.height, gpui::px(96.));
	assert!(capsule.size.width < gpui::px(240.), "content should not fill an arbitrary wide grid");
	assert!(visual.debug_bounds("performance-island-note").is_none());

	ui_performance::preference(Some(false));
}

#[gpui::test]
fn metric_columns_fit_their_content_and_adapt_together_for_wider_values(cx: &mut TestAppContext) {
	ui_performance::preference(Some(true));

	let (island, visual) = cx.add_window_view(|_, cx| {
		let mut island = PerformanceIsland::new(cx);

		island.expansion = Expansion::Clicked;

		island
	});

	visual.update(crate::ui_frame::draw_current_frame);

	let ids = [
		"performance-fps",
		"performance-gpu",
		"performance-cpu",
		"performance-memory",
		"performance-latency",
		"performance-draw",
	];
	let before = visual.debug_bounds("performance-island").unwrap();

	island.update(visual, |s, cx| {
		s.metrics = Metrics {
			fps: 165.,
			draw_ms: 26.5,
			submit_ms: 12_345.4,
			draw_frames: 82,
			ready_samples: 82,
		};
		s.cpu_percent = Some(1_234.);
		s.host.gpu_percent = Some(100.);
		s.host.peak_memory_bytes = Some(12_300_000_000_000.);

		cx.notify();
	});

	visual.update(crate::ui_frame::draw_current_frame);

	thread::sleep(Duration::from_millis(250));

	visual.update(crate::ui_frame::draw_frame);

	let after = visual.debug_bounds("performance-island").unwrap();

	assert!(after.size.width > before.size.width, "wider data should grow its content footprint");

	for id in ids {
		let bounds = visual.debug_bounds(id).unwrap();

		assert!(
			bounds.left() >= after.left() && bounds.right() <= after.right(),
			"{id}: {bounds:?} outside {after:?}"
		);
	}
	for (id, label) in [
		("performance-cpu", "performance-cpu-label"),
		("performance-memory", "performance-memory-label"),
		("performance-draw", "performance-draw-label"),
		("performance-latency", "performance-latency-label"),
	] {
		let label = visual.debug_bounds(label).unwrap();
		let value = visual.debug_bounds(id).unwrap();

		assert_eq!(label.left(), value.left(), "{id}");
		assert_eq!(label.bottom(), value.top(), "{id}: {label:?} overlaps {value:?}");
		assert!(label.right() <= value.right(), "{id}: label exceeds its column");
	}

	ui_performance::preference(Some(false));
}

#[gpui::test]
fn draw_history_keeps_real_samples_in_background_without_inserting_missing_zeros(
	cx: &mut TestAppContext,
) {
	let island = cx.new(PerformanceIsland::new);

	island.update(cx, |island, _| {
		for index in 1..=40 {
			island.record(Metrics { draw_frames: 5, draw_ms: index as f64, ..Default::default() });
			island.record(Metrics::default());
		}

		assert_eq!(island.history.len(), 40);
		assert_eq!(island.history.front().map(|s| s.1), Some(1.));
		assert_eq!(island.history.back().map(|s| s.1), Some(40.));
		assert!(island.history.iter().all(|value| value.1 > 0.));
	});
}

#[gpui::test]
fn metrics_use_only_the_new_histogram_samples(cx: &mut TestAppContext) {
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
fn disabling_the_overlay_stops_sampling_without_enabling_task_traces(cx: &mut TestAppContext) {
	let was_tracing = profiler::trace_enabled();

	ui_performance::preference(Some(true));

	let (island, visual) = cx.add_window_view(|_, cx| PerformanceIsland::new(cx));

	visual.update(crate::ui_frame::draw_current_frame);

	assert!(island.read_with(visual, |s, _| s.task.is_some()));
	assert_eq!(profiler::trace_enabled(), was_tracing);

	ui_performance::preference(Some(false));

	visual.update(|window, cx| {
		cx.refresh_windows();
		window.draw(cx).clear(cx);
	});

	assert!(island.read_with(visual, |s, _| s.task.is_none()));
	assert_eq!(profiler::trace_enabled(), was_tracing);
}
