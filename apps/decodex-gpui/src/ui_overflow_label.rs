//! Shared overflow text: scroll while the window is active, reset when inactive.
use crate::ui_motion;
use gpui::{App, ElementId, IntoElement, RenderOnce, SharedString, TextRun, Window, prelude::*};
use std::time::Instant;

fn offset(elapsed: f32, distance: f32) -> f32 {
	let travel = (distance / 28.).max(0.7);
	let phase = (elapsed - 0.5).max(0.) % (travel * 2. + 2.4);
	let ease = |t: f32| (1. - (t.clamp(0., 1.) * std::f32::consts::PI).cos()) * 0.5;
	if phase < travel {
		distance * ease(phase / travel)
	} else if phase < travel + 1.2 {
		distance
	} else if phase < travel * 2. + 1.2 {
		distance * (1. - ease((phase - travel - 1.2) / travel))
	} else {
		0.
	}
}

#[derive(IntoElement)]
pub(crate) struct OverflowLabel {
	pub id: ElementId,
	pub text: SharedString,
}
impl RenderOnce for OverflowLabel {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let text: SharedString = self.text.replace(['\n', '\r'], " ").into();
		let style = window.text_style();
		let font_size = style.font_size.to_pixels(window.rem_size());
		let run = TextRun {
			len: text.len(),
			font: style.font(),
			color: style.color,
			background_color: None,
			underline: None,
			strikethrough: None,
		};
		let width =
			f32::from(window.text_system().shape_line(text.clone(), font_size, &[run], None).width);
		let active = window.is_window_active();
		let start = window.use_keyed_state(self.id.clone(), cx, |_, _| (Instant::now(), active));
		start.update(cx, |state, _| {
			if !active || state.1 != active {
				state.0 = Instant::now();
			}
			state.1 = active;
		});
		let reduced = ui_motion::reduced();
		let measure_text = text.clone();
		gpui::div()
			.id(self.id)
			.relative()
			.min_w_0()
			.w(gpui::px(width))
			.max_w_full()
			.h(window.line_height())
			.overflow_hidden()
			.child(gpui::div().w_full().whitespace_nowrap().text_ellipsis().opacity(0.).child(text))
			.child(
				gpui::canvas(
					move |_, window, _| {
						let style = window.text_style();
						let run = TextRun {
							len: measure_text.len(),
							font: style.font(),
							color: style.color,
							background_color: None,
							underline: None,
							strikethrough: None,
						};
						window.text_system().shape_line(
							measure_text.clone(),
							style.font_size.to_pixels(window.rem_size()),
							&[run],
							None,
						)
					},
					move |bounds, line, window, cx| {
						let visible = bounds.intersect(&window.content_mask().bounds);
						if visible.size.width <= gpui::px(0.) || visible.size.height <= gpui::px(0.)
						{
							return;
						}
						let distance =
							(f32::from(line.width) - f32::from(visible.size.width)).max(0.);
						let x = if window.is_window_active() && !reduced && distance > 1. {
							ui_motion::request_frame(window, cx);
							offset(start.read(cx).0.elapsed().as_secs_f32(), distance)
						} else {
							0.
						};
						let _ = line.paint(
							bounds.origin - gpui::point(gpui::px(x), gpui::px(0.)),
							window.line_height(),
							gpui::TextAlign::Left,
							None,
							window,
							cx,
						);
					},
				)
				.absolute()
				.top_0()
				.left_0()
				.size_full(),
			)
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn overflow_motion_waits_reaches_the_end_and_returns_without_jumping() {
		let distance = 140.;
		assert_eq!(super::offset(0.49, distance), 0.);
		assert!((super::offset(5.5, distance) - distance).abs() < 0.001);
		assert!((super::offset(6.69, distance) - distance).abs() < 0.001);
		assert!(super::offset(9., distance) < distance);
		assert!(super::offset(11.7, distance) < 0.001);
	}
}
