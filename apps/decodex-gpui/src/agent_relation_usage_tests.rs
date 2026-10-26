use std::{thread, time::Duration};

use gpui::{AppContext as _, Bounds, Entity, Pixels, TestAppContext, VisualTestContext};

use crate::{
	shell::agent_surface::work_board::{
		relations::usage::{AgentSurface, Metric, Sample},
		tests,
	},
	ui_theme::MOTION_PANEL,
};
use decodex_protocol::AgentThreadMetrics;

fn sample(tokens: Option<u64>, ms: Option<u64>) -> Sample {
	Sample {
		tokens,
		stats: Some(AgentThreadMetrics { completed_ms: ms, ..Default::default() }),
		read_at: None,
	}
}

#[test]
fn allocation_units_keep_unknown_values_distinct_from_zero() {
	let unknown = sample(None, None);

	assert_eq!(Metric::Tokens.value(unknown), None);
	assert_eq!(Metric::Time.value(unknown), None);

	let zero = sample(Some(0), Some(0));

	assert_eq!(Metric::Tokens.value(zero), Some(0));
	assert_eq!(Metric::Time.value(zero), Some(0));
	assert_eq!(Metric::Tokens.value_label(None), "Unavailable");
	assert!(Metric::Time.value_label(Some(2_000)).contains("2s"));
}

fn populate(surface: &mut AgentSurface) {
	let rows = surface.factory_rows();

	for (index, row) in rows.iter().enumerate() {
		surface.snapshot.as_mut().unwrap().agent_slots.insert(row.key.clone(), index as u32);
	}

	surface.work_board.usage = Some(
		rows.into_iter()
			.enumerate()
			.map(|(index, row)| {
				let value =
					sample(Some(1_000 + index as u64 * 100), Some(10_000 - index as u64 * 100));

				(row.key, (row.thread, value))
			})
			.collect(),
	);
}

#[gpui::test]
fn switching_units_sorts_by_value_keeps_agent_ids_and_rejects_replaced_thread_counters(
	cx: &mut TestAppContext,
) {
	let surface = cx.new(super::super::super::tests::fixture);

	surface.update(cx, |surface, _| {
		populate(surface);

		let keys = |surface: &AgentSurface| {
			surface
				.relation_usage_entries()
				.into_iter()
				.map(|entry| (entry.row.key, entry.slot))
				.collect::<Vec<_>>()
		};
		let before = keys(surface);
		let descending = |surface: &AgentSurface| {
			assert!(
				surface
					.relation_usage_entries()
					.windows(2)
					.all(|pair| pair[0].value >= pair[1].value)
			);
		};

		descending(surface);

		surface.work_board.view.usage_metric = Metric::Time;

		descending(surface);

		assert_eq!(keys(surface), before.into_iter().rev().collect::<Vec<_>>());

		// Equal recorded values use the persistent agent number as the tie-breaker.
		for (_, value) in surface.work_board.usage.as_mut().unwrap().values_mut() {
			*value = sample(Some(100), Some(1_000));
		}

		assert!(
			surface.relation_usage_entries().windows(2).all(|pair| pair[0].slot <= pair[1].slot)
		);

		let entry = surface.relation_usage_entries().remove(0);

		surface.work_board.usage.as_mut().unwrap().get_mut(&entry.row.key).unwrap().0 =
			Some("replaced".into());

		assert!(
			surface
				.relation_usage_entries()
				.iter()
				.find(|row| row.row.key == entry.row.key)
				.unwrap()
				.value
				.is_none()
		);
	});
}

#[gpui::test]
fn distribution_controls_and_bars_keep_the_chat_and_camera(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut surface = tests::fixture(cx);

		populate(&mut surface);

		surface.workspace.graph_expanded = true;
		surface.work_board.view.camera_fixed = true;
		surface.work_board.view.zoom = Some(0.5);

		surface
	});

	visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));
	visual.update(crate::ui_frame::draw_frame);
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_frame);

	// Resize uses the native monotonic panel clock, not the test dispatcher clock.
	thread::sleep(MOTION_PANEL + Duration::from_millis(40));

	visual.update(crate::ui_frame::draw_frame);

	let before = surface.read_with(visual, |s, _| {
		(s.selected.clone(), s.work_board.view.zoom, s.work_board.view.pan)
	});

	assert!(visual.debug_bounds("graph-usage-time").is_none());

	let options = visual.debug_bounds("relations-usage").unwrap();

	visual.simulate_click(options.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	surface.update(visual, |s, cx| {
		s.escape_interrupt(cx);

		assert!(!s.work_board.view.usage_menu);
	});
	visual.update(crate::ui_frame::draw_frame);
	visual.simulate_click(options.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);

	let time = visual.debug_bounds("graph-usage-time").unwrap();

	visual.simulate_click(time.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, _| {
		assert!(s.work_board.view.usage_metric == Metric::Time);
		assert_eq!((s.selected.clone(), s.work_board.view.zoom, s.work_board.view.pan), before);
	});

	let key = "release".to_owned();

	assert!(visual.debug_bounds("graph-usage-menu").is_none());

	thread::sleep(Duration::from_millis(230));

	visual.update(crate::ui_frame::draw_frame);

	exercise_allocation_selection(&surface, visual, &before, &key);

	assert!(visual.debug_bounds("graph-usage-total").is_none());

	let footer = visual.debug_bounds("graph-usage-footer").unwrap();

	assert_eq!(footer.size.height, gpui::px(32.));

	thread::sleep(Duration::from_millis(230));

	visual.update(crate::ui_frame::draw_frame);

	let release = assert_allocation_proportions(&surface, visual);
	let mark = visual.debug_bounds("graph-allocation-mark-release").unwrap();

	visual.simulate_mouse_move(release.center(), None, Default::default());
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, _| {
		assert_eq!(s.work_board.view.hover.current(std::time::Instant::now()), Some("release"));
		assert_eq!((s.selected.clone(), s.work_board.view.zoom, s.work_board.view.pan), before);
	});

	thread::sleep(Duration::from_millis(230));

	visual.update(crate::ui_frame::draw_frame);

	assert_eq!(visual.debug_bounds("graph-allocation-mark-release").unwrap(), mark);
	assert_eq!(visual.debug_bounds("graph-usage-segment-release").unwrap(), release);

	visual.simulate_mouse_move(gpui::point(gpui::px(1.), gpui::px(1.)), None, Default::default());
	visual.update(crate::ui_frame::draw_frame);

	thread::sleep(Duration::from_millis(140));

	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, _| {
		assert_eq!(s.work_board.view.hover.current(std::time::Instant::now()), None);
		assert_eq!(s.work_board.view.picked_agent.as_ref(), Some(&key));
	});

	surface.update(visual, |s, cx| {
		s.work_board.view.pan = (-9_000., -9_000.);

		s.locate_relation_agent("release", cx);

		let pan = s.work_board.view.pan;

		assert_ne!(pan, (-9_000., -9_000.));

		s.locate_relation_agent("release", cx);

		assert_eq!(s.work_board.view.pan, pan);
		assert_eq!((s.selected.clone(), s.work_board.view.zoom), (before.0.clone(), before.1));
	});
}

fn assert_allocation_proportions(
	surface: &Entity<AgentSurface>,
	visual: &mut VisualTestContext,
) -> Bounds<Pixels> {
	let entries = surface.read_with(visual, |s, _| s.relation_usage_entries());
	let agent = visual.debug_bounds("graph-usage-segment-agent").unwrap();
	let release = visual.debug_bounds("graph-usage-segment-release").unwrap();
	let value = |key: &str| {
		entries.iter().find(|entry| entry.row.key == key).unwrap().value.unwrap() as f64
	};

	assert_eq!(agent.left() < release.left(), value("agent") > value("release"));

	let expected = f32::from(agent.size.width) as f64 * value("release") / value("agent");

	assert!(
		(f32::from(release.size.width) as f64 - expected).abs() <= 1.,
		"allocation must follow recorded values within pixel rounding: {release:?}, {agent:?}, expected {expected}"
	);

	release
}

fn exercise_allocation_selection(
	surface: &Entity<AgentSurface>,
	visual: &mut VisualTestContext,
	before: &(Option<String>, Option<f32>, (f32, f32)),
	key: &str,
) {
	let bar = visual.debug_bounds("graph-usage-segment-release").unwrap();
	let allocation_bounds = visual.debug_bounds("graph-usage-footer").unwrap();

	assert!(
		bar.size.width > gpui::px(0.) && bar.size.height > gpui::px(0.),
		"allocation hit area: {bar:?}"
	);
	assert!(
		allocation_bounds.contains(&bar.center()),
		"allocation must stay in the footer: {bar:?}, {allocation_bounds:?}"
	);

	visual.simulate_mouse_move(bar.center(), None, Default::default());
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, _| {
		assert_eq!(s.work_board.view.hover.current(std::time::Instant::now()), Some("release"));
		assert!(s.work_board.view.picked_agent.is_none(), "hover only previews the relation");
		assert_eq!((s.selected.clone(), s.work_board.view.zoom, s.work_board.view.pan), *before);
	});
	visual.simulate_click(bar.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, _| {
		assert_eq!(s.work_board.view.picked_agent.as_deref(), Some(key), "clicked {bar:?}");
		assert_eq!((s.selected.clone(), s.work_board.view.zoom, s.work_board.view.pan), *before);
	});
}
