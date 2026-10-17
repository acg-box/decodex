//! Stable metric positions, independent of the width of each sampled number.
use super::{PerformanceIsland, bytes, chart, timing};
use crate::ui_theme::{AMBER, BLUE, CAPTION_SIZE, TEXT_MUTED};
use gpui::{AnyElement, FontFeatures, FontWeight, IntoElement, prelude::*};
use std::sync::{Arc, OnceLock};

pub(super) fn figures() -> FontFeatures {
	static FEATURES: OnceLock<FontFeatures> = OnceLock::new();
	FEATURES.get_or_init(|| FontFeatures(Arc::new(vec![("tnum".into(), 1)]))).clone()
}

fn number(id: &'static str, value: String) -> gpui::Div {
	gpui::div()
		.debug_selector(move || format!("performance-{id}"))
		.flex_none()
		.text_right()
		.whitespace_nowrap()
		.font_weight(FontWeight::MEDIUM)
		.child(value)
}

pub(super) fn header(island: &PerformanceIsland, fps: String) -> AnyElement {
	let gpu = island.host.gpu_percent.map_or_else(|| "—".into(), |v| format!("{v:.0}%"));
	gpui::div()
		.w_full()
		.px_3()
		.flex_none()
		.flex()
		.items_center()
		.justify_between()
		.child(
			gpui::div()
				.flex()
				.items_baseline()
				.gap_1()
				.child(
					gpui::div()
						.debug_selector(|| "performance-fps-label".into())
						.text_color(gpui::rgb(TEXT_MUTED))
						.child("FPS"),
				)
				.child(number("fps", fps).w(gpui::px(26.))),
		)
		.child(
			gpui::div()
				.flex()
				.items_baseline()
				.gap_1()
				.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child("System GPU"))
				.child(number("gpu", gpu).w(gpui::px(32.))),
		)
		.into_any_element()
}

pub(super) fn details(
	island: &PerformanceIsland,
	window: &mut gpui::Window,
	cx: &mut gpui::App,
) -> AnyElement {
	let rows = [
		("cpu", "CPU", island.cpu_percent.map_or_else(|| "—".into(), |v| format!("{v:.0}%"))),
		("memory", "Peak RAM", bytes(island.host.peak_memory_bytes)),
		("latency", "Latency p95", timing(island.metrics.submit_ms, island.metrics.ready_samples)),
	];
	gpui::div()
		.debug_selector(|| "performance-island-details".into())
		.w_full()
		.px_3()
		.pb(gpui::px(6.))
		.flex_none()
		.flex()
		.flex_col()
		.gap_1()
		.child(
			gpui::div()
				.w_full()
				.grid()
				.grid_cols(3)
				.gap_x_3()
				.children(rows.into_iter().map(|(id, label, value)| metric(id, label, value))),
		)
		.child(
			gpui::div()
				.w_full()
				.h(gpui::px(28.))
				.flex_none()
				.flex()
				.items_center()
				.gap_2()
				.child(
					metric(
						"draw",
						"Draw p95",
						timing(island.metrics.draw_ms, island.metrics.draw_frames),
					)
					.w(gpui::px(66.))
					.when(
						island.display_hz.is_some_and(|hz| island.metrics.draw_ms > 1_000. / hz),
						|d| d.text_color(gpui::rgb(AMBER)),
					),
				)
				.child(chart::render(&island.history, window, cx)),
		)
		.into_any_element()
}

fn metric(id: &'static str, label: &'static str, value: String) -> gpui::Div {
	gpui::div()
		.debug_selector(move || format!("performance-{id}-row"))
		.min_w_0()
		.h(gpui::px(28.))
		.flex_none()
		.flex()
		.flex_col()
		.whitespace_nowrap()
		.child(
			gpui::div()
				.debug_selector(move || format!("performance-{id}-label"))
				.text_size(gpui::px(CAPTION_SIZE))
				.line_height(gpui::px(12.))
				.text_color(gpui::rgb(if id == "draw" { BLUE } else { TEXT_MUTED }))
				.child(label),
		)
		.child(number(id, value).w_full().text_left())
}
