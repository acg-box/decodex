//! A bounded CPU draw-time sparkline with a display-budget reference.
use crate::{
	ui_motion,
	ui_theme::{AMBER, BLUE},
};
use gpui::{
	AnyElement, IntoElement, PathBuilder,
	prelude::{InteractiveElement as _, ParentElement as _, Styled as _},
};
use std::{collections::VecDeque, time::Duration};

pub(super) fn render(
	history: &VecDeque<f64>,
	budget: Option<f64>,
	age: Duration,
	period: Duration,
	window: &mut gpui::Window,
	cx: &mut gpui::App,
) -> AnyElement {
	let samples: Vec<_> = history.iter().copied().collect();
	let target = samples.iter().copied().fold(budget.unwrap_or(16.67), f64::max) * 1.15;
	let maximum = f64::from(ui_motion::value("performance-chart-scale", target as f32, window, cx));
	let phase =
		if ui_motion::reduced() { 1. } else { (age.as_secs_f32() / period.as_secs_f32()).min(1.) };
	if samples.len() > 1 && phase < 1. {
		ui_motion::request_frame(window, cx);
	}
	gpui::div()
		.debug_selector(|| "performance-island-chart".into())
		.w_full()
		.h(gpui::px(32.))
		.overflow_hidden()
		.child(
			gpui::canvas(
				|_, _, _| (),
				move |bounds, _, window, _| {
					let height = f32::from(bounds.size.height) - 4.;
					let width = f32::from(bounds.size.width) - 4.;
					let point = |x, ms: f64| {
						bounds.origin
							+ gpui::point(
								gpui::px(x + 2.),
								gpui::px(2. + height * (1. - (ms / maximum) as f32)),
							)
					};
					if let Some(budget) = budget {
						let mut reference = PathBuilder::stroke(gpui::px(1.));
						reference.move_to(point(0., budget));
						reference.line_to(point(width, budget));
						if let Ok(path) = reference.build() {
							window.paint_path(path, gpui::rgba(0xffffff28));
						}
					}
					let mut line = PathBuilder::stroke(gpui::px(1.8));
					let mut last: Option<gpui::Point<gpui::Pixels>> = None;
					for (index, ms) in samples.iter().enumerate() {
						let x = sample_x(index, samples.len(), width, phase);
						let next = point(x, *ms);
						if let Some(previous) = last {
							let middle = (previous.x + next.x) / 2.;
							line.cubic_bezier_to(
								next,
								gpui::point(middle, previous.y),
								gpui::point(middle, next.y),
							);
						} else {
							line.move_to(next);
						}
						last = Some(next);
					}
					if let Ok(path) = line.build() {
						let color = if samples
							.last()
							.zip(budget)
							.is_some_and(|(ms, budget)| *ms > budget)
						{
							AMBER
						} else {
							BLUE
						};
						window.paint_path(path, gpui::rgb(color));
					}
				},
			)
			.size_full(),
		)
		.into_any_element()
}

fn sample_x(index: usize, length: usize, width: f32, phase: f32) -> f32 {
	width * ((32 - length + index) as f32 + 1. - phase) / 31.
}

#[cfg(test)]
mod tests {
	#[test]
	fn a_new_sample_keeps_every_retained_point_at_its_current_position() {
		for count in [2, 12, 31, 32] {
			for index in usize::from(count == 32)..count {
				let before = super::sample_x(index, count, 278., 1.);
				let next_index = index - usize::from(count == 32);
				let after = super::sample_x(next_index, (count + 1).min(32), 278., 0.);
				assert!((before - after).abs() < 0.001);
			}
		}
	}
}
