//! Stable metric positions, independent of the width of each sampled number.
use super::{PerformanceIsland, bytes, chart, timing};
use crate::ui_theme::{AMBER, TEXT_MUTED};
use gpui::{AnyElement, FontFeatures, IntoElement, prelude::*};
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
		.child(value)
}

pub(super) fn header(island: &PerformanceIsland, fps: String) -> AnyElement {
	let gpu = island.host.gpu_percent.map_or_else(|| "—".into(), |v| format!("{v:.0}%"));
	gpui::div()
		.w(gpui::px(184.))
		.flex_none()
		.flex()
		.items_center()
		.justify_between()
		.child(gpui::div().flex().gap_1().child(number("fps", fps).w(gpui::px(26.))).child("FPS"))
		.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child("·"))
		.child(
			gpui::div()
				.flex()
				.gap_1()
				.child("System GPU")
				.child(number("gpu", gpu).w(gpui::px(32.))),
		)
		.into_any_element()
}

pub(super) fn details(
	island: &PerformanceIsland,
	window: &mut gpui::Window,
	cx: &mut gpui::App,
) -> AnyElement {
	let budget = island
		.display_hz
		.map_or_else(|| "—".into(), |hz| format!("{hz:.0} Hz · {:.2} ms", 1_000. / hz));
	let rows = [
		("cpu", "CPU", island.cpu_percent.map_or_else(|| "—".into(), |v| format!("{v:.0}%"))),
		("memory", "Peak RAM", bytes(island.host.peak_memory_bytes)),
		("latency", "Latency p95", timing(island.metrics.submit_ms, island.metrics.ready_samples)),
	];
	gpui::div()
		.debug_selector(|| "performance-island-details".into())
		.w(gpui::px(302.))
		.px_3()
		.pb_2()
		.flex_none()
		.flex()
		.flex_col()
		.gap_1()
		.child(gpui::div().w_full().grid().grid_cols(3).children(rows.into_iter().map(
			|(id, label, value)| {
				gpui::div()
					.min_w_0()
					.flex()
					.flex_col()
					.child(
						gpui::div()
							.line_height(gpui::px(12.))
							.text_color(gpui::rgb(TEXT_MUTED))
							.child(label),
					)
					.child(number(id, value).w_full().text_left())
			},
		)))
		.child(
			gpui::div()
				.h(gpui::px(16.))
				.flex_none()
				.flex()
				.items_center()
				.justify_between()
				.child(
					gpui::div().flex().gap_1().child("Draw p95").child(
						number("draw", timing(island.metrics.draw_ms, island.metrics.draw_frames))
							.w(gpui::px(52.))
							.text_left()
							.when(
								island
									.display_hz
									.is_some_and(|hz| island.metrics.draw_ms > 1_000. / hz),
								|d| d.text_color(gpui::rgb(AMBER)),
							),
					),
				)
				.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(budget)),
		)
		.child(chart::render(&island.history, window, cx))
		.into_any_element()
}
