//! Frame-paced presentation of received text. Never delays transport or stores output.
use std::time::Instant;

use gpui::{App, RenderOnce};

use crate::{
	shell::agent_surface::{IntoElement, SharedString, Window, markdown},
	ui_motion,
};

#[derive(gpui::IntoElement)]
pub(super) struct StreamingText {
	pub text: String,
	pub key: String,
	pub already_visible: bool,
}
impl RenderOnce for StreamingText {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(SharedString::from(self.key.clone()), cx, |_, _| {
			if self.already_visible {
				ui_motion::TextReveal::settled(&self.text, Instant::now())
			} else {
				ui_motion::TextReveal::new(Instant::now())
			}
		});
		let (end, moving) = state.update(cx, |state, _| state.sample(&self.text, Instant::now()));

		if moving {
			ui_motion::request_frame(window, cx);
		}

		markdown::render(&self.text[..end], &self.key)
	}
}
