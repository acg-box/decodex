//! Quiet agent state and hover-only overflow motion shared by navigation surfaces.
use crate::{ui_motion, ui_theme};
use gpui::{App, ElementId, IntoElement, RenderOnce, SharedString, TextRun, Window, prelude::*};
use std::time::Instant;

#[derive(Default)]
struct ScrollState {
	width: f32,
	hovered: Option<Instant>,
	returning: Option<(Instant, f32)>,
	offset: f32,
}

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
pub(crate) struct AgentLabel {
	pub id: ElementId,
	pub text: SharedString,
}
impl RenderOnce for AgentLabel {
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
		let state = window.use_keyed_state(self.id.clone(), cx, |_, _| ScrollState::default());
		let reduced = ui_motion::reduced();
		let (x, moving) = state.update(cx, |s, _| {
			let distance = (width - s.width).max(0.);
			let moving = !reduced && distance > 1. && s.hovered.is_some();
			s.offset = if reduced || distance <= 1. {
				s.returning = None;
				0.
			} else if let Some(start) = s.hovered {
				offset(start.elapsed().as_secs_f32(), distance)
			} else if let Some((start, from)) = s.returning {
				let t = (start.elapsed().as_secs_f32() / 0.22).min(1.);
				if t >= 1. {
					s.returning = None;
				}
				from * (1. - t).powi(3)
			} else {
				0.
			};
			(s.offset, moving || s.returning.is_some())
		});
		if moving {
			ui_motion::request_frame(window, cx);
		}
		let measure = state.clone();
		gpui::div()
			.id(self.id)
			.relative()
			.min_w_0()
			.w(gpui::px(width))
			.max_w_full()
			.h(window.line_height())
			.overflow_hidden()
			.on_hover(move |hovered, _, cx| {
				state.update(cx, |s, cx| {
					s.hovered = hovered.then(Instant::now);
					s.returning = (!hovered && s.offset > 0.).then(|| (Instant::now(), s.offset));
					cx.notify();
				})
			})
			.child(
				gpui::canvas(
					move |bounds, _, cx| {
						let width = f32::from(bounds.size.width);
						measure.update(cx, |s, cx| {
							if (s.width - width).abs() > 0.5 {
								s.width = width;
								cx.notify();
							}
						});
					},
					|_, _, _, _| {},
				)
				.absolute()
				.size_full(),
			)
			.child(if x > 0.01 {
				gpui::div()
					.absolute()
					.left(gpui::px(-x))
					.w(gpui::px(width))
					.whitespace_nowrap()
					.child(text)
			} else {
				gpui::div().w_full().whitespace_nowrap().text_ellipsis().child(text)
			})
	}
}

#[derive(IntoElement)]
pub(crate) struct AgentSignal {
	pub id: ElementId,
	pub state: String,
}
impl RenderOnce for AgentSignal {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let signal = match self.state.as_str() {
			"Running" | "Starting" | "active" => Some((ui_theme::BLUE, true, "Working")),
			"Needs you" => Some((ui_theme::AMBER, false, "Waiting for your decision")),
			"Needs attention" | "Blocked" => Some((ui_theme::AMBER, false, "Needs attention")),
			"systemError" | "failed" => Some((0xe67676, false, "Execution failed")),
			_ => None,
		};
		let mut slot = gpui::div()
			.id(self.id.clone())
			.w(gpui::px(10.))
			.h(gpui::px(16.))
			.flex_none()
			.flex()
			.items_center()
			.justify_center();
		if let Some((color, pulse, label)) = signal {
			let opacity = if pulse && !ui_motion::reduced() {
				let start = window.use_keyed_state(self.id, cx, |_, _| Instant::now());
				ui_motion::request_frame(window, cx);
				0.65 + 0.35 * (start.read(cx).elapsed().as_secs_f32() * 2.5).sin()
			} else {
				1.
			};
			slot = slot
				.aria_label(label)
				.tooltip(move |_, cx| cx.new(|_| SignalTip(label)).into())
				.child(
					gpui::div()
						.size(gpui::px(5.))
						.rounded_full()
						.bg(gpui::rgb(color))
						.opacity(opacity),
				);
		}
		slot
	}
}
struct SignalTip(&'static str);
impl gpui::Render for SignalTip {
	fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
		gpui::div()
			.px_2()
			.py_1()
			.rounded(gpui::px(6.))
			.bg(gpui::rgb(0x27272b))
			.text_size(gpui::px(11.))
			.child(self.0)
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
