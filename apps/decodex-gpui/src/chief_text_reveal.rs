//! Frame-paced presentation of received text. Never delays transport or stores output.
use super::*;
use std::time::Instant;
use unicode_segmentation::UnicodeSegmentation;

struct Reveal {
	text: String,
	ends: Vec<usize>,
	shown: f32,
	tick: Instant,
}
impl Reveal {
	fn new(now: Instant) -> Self {
		Self { text: String::new(), ends: Vec::new(), shown: 0., tick: now }
	}

	fn sample(&mut self, text: &str, now: Instant) -> (usize, bool) {
		if self.text != text {
			if !text.starts_with(&self.text) {
				// Provider corrections replace the visible prefix without replaying it.
				self.shown = text.graphemes(true).count() as f32;
			}
			self.text = text.into();
			self.ends = text.grapheme_indices(true).map(|(i, g)| i + g.len()).collect();
		}
		let elapsed = now.duration_since(self.tick).as_secs_f32().min(0.05);
		self.tick = now;
		let remaining = self.ends.len() as f32 - self.shown;
		// Small deltas appear immediately; bursts catch up in a short bounded tail.
		let speed = 90_f32.max(remaining / 0.12);
		self.shown = (self.shown + elapsed * speed).min(self.ends.len() as f32);
		if remaining > 0. && self.shown < 1. {
			self.shown = 1.;
		}
		let count = self.shown.floor() as usize;
		(
			count.checked_sub(1).and_then(|i| self.ends.get(i)).copied().unwrap_or(0),
			count < self.ends.len(),
		)
	}
}

#[derive(gpui::IntoElement)]
pub(super) struct StreamingText {
	pub text: String,
	pub key: String,
}
impl gpui::RenderOnce for StreamingText {
	fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
		let state = window.use_keyed_state(SharedString::from(self.key.clone()), cx, |_, _| {
			Reveal::new(Instant::now())
		});
		let (end, moving) = state.update(cx, |state, _| state.sample(&self.text, Instant::now()));
		if moving {
			crate::ui_motion::request_frame(window, cx);
		}
		markdown::render(&self.text[..end], &self.key)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;
	#[test]
	fn bursts_reveal_on_frames_without_splitting_graphemes_and_then_stop() {
		let now = Instant::now();
		let mut reveal = Reveal::new(now);
		let text = "你好👨‍👩‍👧‍👦e\u{301}".repeat(40);
		let (first, moving) = reveal.sample(&text, now);
		assert!(moving);
		assert_eq!(&text[..first], "你");
		let mut previous = first;
		for frame in 1..=120 {
			let (end, _) = reveal.sample(&text, now + Duration::from_millis(frame * 8));
			assert!(end >= previous);
			assert!(end == text.len() || text.grapheme_indices(true).any(|(i, _)| i == end));
			previous = end;
		}
		assert_eq!(previous, text.len());
		assert!(!reveal.sample(&text, now + Duration::from_secs(2)).1);
	}
	#[test]
	fn corrections_do_not_replay_or_slice_invalid_text() {
		let now = Instant::now();
		let mut reveal = Reveal::new(now);
		reveal.sample("Original text", now);
		assert_eq!(reveal.sample("修正后的内容", now).0, "修正后的内容".len());
		assert_eq!(reveal.sample("", now), (0, false));
	}
}
