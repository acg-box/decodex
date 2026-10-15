//! Frame-paced presentation of received text. Never delays transport or stores output.
use std::time::Instant;

use gpui::{App, RenderOnce};

use crate::{
	shell::agent_surface::{IntoElement, SharedString, Window, markdown},
	ui_motion,
};

#[derive(gpui::IntoElement)]
pub(super) struct StreamingText {
	text: String,
	key: String,
	output: bool,
	complete: bool,
}
impl StreamingText {
	pub(super) fn output(text: String, key: String, complete: bool) -> Self {
		Self { text, key, output: true, complete }
	}

	pub(super) fn caption(text: String, key: String) -> Self {
		Self { text, key, output: false, complete: false }
	}
}
impl RenderOnce for StreamingText {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(SharedString::from(self.key.clone()), cx, |_, _| {
			if self.output {
				ui_motion::TextReveal::output(&self.text, self.complete, Instant::now())
			} else {
				ui_motion::TextReveal::new(Instant::now())
			}
		});
		let (end, moving) = state.update(cx, |state, _| {
			let now = Instant::now();
			let sample = state.sample(&self.text, now);
			if self.complete {
				state.finish(now);
			}
			sample
		});

		if moving {
			ui_motion::request_frame(window, cx);
		}

		markdown::render(&self.text[..end], &self.key)
	}
}
