//! Quiet agent state for the ownership sidebar.
use crate::{ui_motion, ui_theme};
use gpui::{App, ElementId, IntoElement, RenderOnce, Window, prelude::*};
use std::time::Instant;

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
