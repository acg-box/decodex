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
		let line = OverflowLine::new(self.id.clone(), self.text, window.text_style(), window, cx);
		gpui::div()
			.id(self.id)
			.relative()
			.min_w_0()
			.w(line.width())
			.max_w_full()
			.h(line.height)
			.overflow_hidden()
			.child(
				gpui::canvas(
					|_, _, _| (),
					move |bounds, _, window, cx| line.paint(bounds, window, cx),
				)
				.absolute()
				.top_0()
				.left_0()
				.size_full(),
			)
	}
}

/// Shared shaping and motion for labels inside ordinary layouts or fixed canvases.
/// The canvas path keeps graph headers out of repeated intrinsic-size layout.
pub(crate) struct OverflowLine {
	line: gpui::ShapedLine,
	pub height: gpui::Pixels,
	start: gpui::Entity<(Instant, bool)>,
	reduced: bool,
}
impl OverflowLine {
	pub(crate) fn new(
		id: ElementId,
		text: SharedString,
		style: gpui::TextStyle,
		window: &mut Window,
		cx: &mut App,
	) -> Self {
		let text: SharedString = text.replace(['\n', '\r'], " ").into();
		let run = TextRun {
			len: text.len(),
			font: style.font(),
			color: style.color,
			background_color: None,
			underline: None,
			strikethrough: None,
		};
		let line = window.text_system().shape_line(
			text,
			style.font_size.to_pixels(window.rem_size()),
			&[run],
			None,
		);
		let active = ui_motion::application_active();
		let start = window.use_keyed_state(id, cx, |_, _| (Instant::now(), active));
		start.update(cx, |state, _| {
			if !active || state.1 != active {
				state.0 = Instant::now();
			}
			state.1 = active;
		});
		Self {
			line,
			height: style.line_height_in_pixels(window.rem_size()),
			start,
			reduced: ui_motion::reduced(),
		}
	}

	pub(crate) fn width(&self) -> gpui::Pixels {
		self.line.width
	}

	pub(crate) fn paint(
		&self,
		bounds: gpui::Bounds<gpui::Pixels>,
		window: &mut Window,
		cx: &mut App,
	) {
		let visible = bounds.intersect(&window.content_mask().bounds);
		if visible.is_empty() {
			return;
		}
		let distance = (f32::from(self.line.width) - f32::from(visible.size.width)).max(0.);
		let x = if ui_motion::application_active() && !self.reduced && distance > 1. {
			ui_motion::request_frame(window, cx);
			offset(self.start.read(cx).0.elapsed().as_secs_f32(), distance)
		} else {
			0.
		};
		window.with_content_mask(Some(gpui::ContentMask { bounds: visible }), |window| {
			window.paint_layer(visible, |window| {
				let _ = self.line.paint(
					bounds.origin - gpui::point(gpui::px(x), gpui::px(0.)),
					self.height,
					gpui::TextAlign::Left,
					None,
					window,
					cx,
				);
			});
		});
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
