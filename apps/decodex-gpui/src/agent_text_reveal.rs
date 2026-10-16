//! Frame-paced presentation of received text. Never delays transport or stores output.
use std::{cell::Cell, rc::Rc, time::Instant};

use gpui::{App, RenderOnce, prelude::*};

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
	settled: Option<Rc<Cell<bool>>>,
}
impl StreamingText {
	pub(super) fn output(text: String, key: String, complete: bool) -> Self {
		Self { text, key, output: true, complete, settled: None }
	}

	pub(super) fn caption(text: String, key: String) -> Self {
		Self { text, key, output: false, complete: false, settled: None }
	}

	pub(super) fn settled(mut self, signal: Rc<Cell<bool>>) -> Self {
		self.settled = Some(signal);
		self
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

		let content = markdown::render(&self.text[..end], &self.key);
		let flow = window.use_keyed_state(
			SharedString::from(format!("text-flow-{}", self.key)),
			cx,
			|_, _| Flow::new(self.output && !self.complete),
		);
		if !flow.read(cx).enabled {
			if let Some(signal) = self.settled {
				signal.set(!moving && end == self.text.len());
			}
			return content;
		}
		let (height, growing) = flow.read(cx).sample(Instant::now());
		if let Some(signal) = &self.settled {
			signal.set(!moving && !growing && end == self.text.len());
		}
		if growing {
			ui_motion::request_frame(window, cx);
		}
		// The row owns its displayed height. Bottom-follow uses this same layout,
		// so a line wrap cannot jump the scroll position by a full line at once.
		gpui::div()
			.w_full()
			.min_w_0()
			.flex_none()
			.overflow_hidden()
			.when_some(height, |d, height| d.h(gpui::px(height)))
			.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.flex_none()
					.on_children_prepainted(move |bounds, window, cx| {
						let Some(bounds) = bounds.first() else { return };
						let moving = flow.update(cx, |s, _| {
							s.measure(
								bounds.size.width.into(),
								bounds.size.height.into(),
								Instant::now(),
							);
							s.sample(Instant::now()).1
						});
						if moving {
							if let Some(signal) = &self.settled {
								signal.set(false);
							}
							ui_motion::request_frame(window, cx);
						}
					})
					.child(content),
			)
			.into_any_element()
	}
}

struct Flow {
	enabled: bool,
	width: f32,
	height: Option<ui_motion::Tween>,
}
impl Flow {
	fn new(enabled: bool) -> Self {
		Self { enabled, width: 0., height: None }
	}

	fn sample(&self, now: Instant) -> (Option<f32>, bool) {
		(
			self.height.as_ref().map(|h| h.sample(now)),
			self.height.as_ref().is_some_and(|h| h.moving(now)),
		)
	}

	fn measure(&mut self, width: f32, height: f32, now: Instant) {
		if self.height.is_none() || (self.width - width).abs() > 0.5 {
			// First paint and width changes use natural geometry without replay.
			self.width = width;
			self.height = Some(ui_motion::Tween::new(height));
		} else if let Some(motion) = &mut self.height {
			motion.target(height, now);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;
	#[test]
	fn streamed_lines_grow_monotonically_and_resizing_uses_natural_height() {
		for frame in [6, 16] {
			let now = Instant::now();
			let mut flow = Flow::new(true);
			flow.measure(400., 24., now);
			let mut previous = 24.;
			for ms in (0..600).step_by(frame) {
				let at = now + Duration::from_millis(ms as u64);
				let target = if ms < 80 { 48. } else { 72. };
				flow.measure(400., target, at);
				let value = flow.sample(at).0.unwrap();
				assert!(value >= previous && value <= target);
				assert!(value - previous < 12., "a new line must not jump the viewport");
				previous = value;
			}
			assert_eq!(previous, 72.);
			flow.measure(200., 144., now + Duration::from_secs(1));
			assert_eq!(flow.sample(now + Duration::from_secs(1)), (Some(144.), false));
		}
	}
}
