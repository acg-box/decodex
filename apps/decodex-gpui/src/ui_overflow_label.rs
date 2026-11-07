//! Shared overflow text follows the application rendering policy.
use std::{f32::consts::PI, time::Instant};

use gpui::{
	App, Bounds, ContentMask, ElementId, Entity, IntoElement, Pixels, RenderOnce, ShapedLine,
	SharedString, TextAlign, TextRun, TextStyle, Window,
	prelude::{InteractiveElement as _, ParentElement as _, Styled as _},
};

use crate::ui_motion;

#[derive(IntoElement)]
pub(crate) struct OverflowLabel {
	pub id: ElementId,
	pub text: SharedString,
}
impl RenderOnce for OverflowLabel {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		HoverOverflowLabel { id: self.id, text: self.text, active: true }.render(window, cx)
	}
}
#[derive(IntoElement)]
pub(crate) struct HoverOverflowLabel {
	pub id: ElementId,
	pub text: SharedString,
	pub active: bool,
}
impl RenderOnce for HoverOverflowLabel {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let line = OverflowLine::new_active(
			self.id.clone(),
			self.text,
			window.text_style(),
			self.active,
			window,
			cx,
		);

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
	line: ShapedLine,
	pub height: Pixels,
	start: Entity<Motion>,
	reduced: bool,
}
struct Motion {
	started: Instant,
	active: bool,
	offset: f32,
	returning: ui_motion::Tween,
}
impl OverflowLine {
	pub(crate) fn new_active(
		id: ElementId,
		text: SharedString,
		style: TextStyle,
		enabled: bool,
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
		let active = enabled && !ui_motion::reduced();
		let start = window.use_keyed_state(id, cx, |_, _| Motion {
			started: Instant::now(),
			active,
			offset: 0.,
			returning: ui_motion::Tween::new(0.),
		});

		start.update(cx, |state, _| {
			if state.active != active {
				state.started = Instant::now();
				state.returning = ui_motion::Tween::new(state.offset);
				state.returning.target(0., Instant::now());
				state.active = active;
			}
		});

		Self {
			line,
			height: style.line_height_in_pixels(window.rem_size()),
			start,
			reduced: ui_motion::reduced(),
		}
	}

	pub(crate) fn width(&self) -> Pixels {
		self.line.width
	}

	pub(crate) fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
		let visible = bounds.intersect(&window.content_mask().bounds);

		if visible.is_empty() {
			return;
		}

		let distance = (f32::from(self.line.width) - f32::from(visible.size.width)).max(0.);
		let state = self.start.read(cx);
		let active = state.active;
		let elapsed = state.started.elapsed().as_secs_f32();
		let returning = state.returning.sample(Instant::now());
		let moving = state.returning.moving(Instant::now());
		let x = if self.reduced {
			0.
		} else if active && distance > 1. {
			ui_motion::request_frame(window, cx);
			offset(elapsed, distance)
		} else {
			let value = returning;
			if moving {
				ui_motion::request_frame(window, cx);
			}
			value
		};
		self.start.update(cx, |state, _| state.offset = x);

		window.with_content_mask(Some(ContentMask { bounds: visible }), |window| {
			window.paint_layer(visible, |window| {
				let _ = self.line.paint(
					bounds.origin - gpui::point(gpui::px(x), gpui::px(0.)),
					self.height,
					TextAlign::Left,
					None,
					window,
					cx,
				);
			});
		});
	}
}

fn offset(elapsed: f32, distance: f32) -> f32 {
	let travel = (distance / 28.).max(0.7);
	let phase = (elapsed - 0.5).max(0.) % (travel * 2. + 2.4);
	let ease = |t: f32| (1. - (t.clamp(0., 1.) * PI).cos()) * 0.5;

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
