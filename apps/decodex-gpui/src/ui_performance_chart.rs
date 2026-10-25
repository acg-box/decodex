//! A time-based sparkline. Sampling never restarts the presentation clock.
use std::{
	collections::VecDeque,
	time::{Duration, Instant},
};

use gpui::{
	AnyElement, App, Bounds, IntoElement, PathBuilder, Window,
	prelude::{InteractiveElement as _, ParentElement as _, Styled as _},
};

use crate::{
	ui_motion,
	ui_theme::{self, BLUE},
};

pub(super) type History = VecDeque<(Instant, f64)>;

const WINDOW: Duration = Duration::from_secs(16);
// Leave two normal samples in hand, so the head can reveal measured data.
const DELAY: Duration = Duration::from_secs(1);
const CEILING_MS: f64 = 32.;

pub(super) fn record(history: &mut History, at: Instant, ms: f64) {
	history.push_back((at, ms));

	// Retain guard points beyond both edges of the 16-second viewport.
	if history.len() > 40 {
		history.pop_front();
	}
}

pub(super) fn render(history: &History, window: &mut Window, cx: &mut App) -> AnyElement {
	let now = Instant::now();
	let reduced = ui_motion::reduced();
	let end = if reduced { history.back().map_or(now, |sample| sample.0) } else { now - DELAY };

	if !reduced && history.len() > 1 && history.back().is_some_and(|s| now - s.0 < WINDOW + DELAY) {
		ui_motion::request_frame(window, cx);
	}

	let samples: Vec<_> = history.iter().copied().collect();

	gpui::div()
		.debug_selector(|| "performance-island-chart".into())
		.flex_1()
		.min_w_0()
		.h(gpui::px(28.))
		.overflow_hidden()
		.child(
			gpui::canvas(
				|_, _, _| (),
				move |bounds, _, window, _| {
					let width = f32::from(bounds.size.width);
					let height = f32::from(bounds.size.height) - 4.;
					let point = |(at, ms): (Instant, f64)| {
						bounds.origin
							+ gpui::point(
								gpui::px(sample_x(at, end, width)),
								gpui::px(
									2. + height * (1. - (ms / CEILING_MS).clamp(0., 1.) as f32),
								),
							)
					};
					let mut line = PathBuilder::stroke(gpui::px(1.8));
					let mut connected = false;

					for pair in samples.windows(2) {
						let previous = point(pair[0]);
						let next = point(pair[1]);

						if next.x < bounds.left()
							|| previous.x > bounds.right()
							|| pair[1].0 - pair[0].0 > Duration::from_secs(2)
						{
							connected = false;

							continue;
						}
						if !connected {
							line.move_to(previous);
						}

						let middle = (previous.x + next.x) / 2.;

						line.cubic_bezier_to(
							next,
							gpui::point(middle, previous.y),
							gpui::point(middle, next.y),
						);

						connected = true;
					}

					if let Ok(path) = line.build() {
						window.paint_path(path, ui_theme::rgb(BLUE));
					}

					// Fade at fixed screen edges, not at sample boundaries.
					for (x, angle, width) in [(0., 90., 12.), (width - 8., 270., 8.)] {
						window.paint_quad(gpui::fill(
							Bounds::new(
								bounds.origin + gpui::point(gpui::px(x), gpui::px(0.)),
								gpui::size(gpui::px(width), bounds.size.height),
							),
							gpui::linear_gradient(
								angle,
								gpui::linear_color_stop(ui_theme::rgb(0x141418), 0.),
								gpui::linear_color_stop(ui_theme::rgba(0x14141800), 1.),
							),
						));
					}
				},
			)
			.size_full(),
		)
		.into_any_element()
}

fn sample_x(at: Instant, end: Instant, width: f32) -> f32 {
	let age = if at <= end { -(end - at).as_secs_f32() } else { (at - end).as_secs_f32() };

	width * (1. + age / WINDOW.as_secs_f32())
}

#[cfg(test)]
mod tests {

	use crate::ui_performance::chart::{self, DELAY, Duration, History, Instant, WINDOW};

	#[test]
	fn irregular_sampling_never_moves_an_existing_point_or_removes_the_visible_tail() {
		let start = Instant::now();
		let mut history = History::new();

		for index in 0..40 {
			chart::record(&mut history, start + Duration::from_millis(index * 510), index as f64);
		}

		let end = start + Duration::from_millis(20_000) - DELAY;
		let before: Vec<_> =
			history.iter().map(|s| (s.0, chart::sample_x(s.0, end, 278.))).collect();

		chart::record(&mut history, start + Duration::from_millis(20_491), 12.);

		assert_eq!(history.len(), 40);
		assert!(before[0].1 < 0., "only an offscreen guard point can be evicted");

		for (at, x) in before.into_iter().skip(1) {
			assert_eq!(x, chart::sample_x(at, end, 278.));
			assert!(chart::sample_x(at, end + Duration::from_millis(6), 278.) < x);
		}
	}

	#[test]
	fn measured_head_arrives_before_it_enters_the_viewport() {
		let now = Instant::now();
		let end = now - DELAY;

		assert!(chart::sample_x(now, end, 278.) > 278.);
		assert_eq!(chart::sample_x(end, end, 278.), 278.);
		assert_eq!(chart::sample_x(end - WINDOW, end, 278.), 0.);
	}
}
