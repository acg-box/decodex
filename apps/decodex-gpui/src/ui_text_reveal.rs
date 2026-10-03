//! Shared grapheme-safe presentation for model output, live captions and dictation.
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation as _;

pub(crate) struct TextReveal {
	text: String,
	ends: Vec<usize>,
	shown: f32,
	tick: Instant,
	deadline: Instant,
}
impl TextReveal {
	pub(crate) fn new(now: Instant) -> Self {
		Self { text: String::new(), ends: Vec::new(), shown: 0., tick: now, deadline: now }
	}

	pub(crate) fn settled(text: &str, now: Instant) -> Self {
		let mut state = Self::new(now);
		state.text = text.into();
		state.ends = text.grapheme_indices(true).map(|(i, g)| i + g.len()).collect();
		state.shown = state.ends.len() as f32;
		state
	}

	pub(crate) fn sample(&mut self, text: &str, now: Instant) -> (usize, bool) {
		if self.text != text {
			if self.shown >= self.ends.len() as f32 {
				self.tick = now;
			}
			self.deadline = now + Duration::from_millis(120);
			if !text.starts_with(&self.text) {
				// Provider corrections replace the visible prefix without replaying it.
				self.shown = text.graphemes(true).count() as f32;
			}

			self.text = text.into();
			self.ends = text.grapheme_indices(true).map(|(i, g)| i + g.len()).collect();
		}

		let elapsed = now.saturating_duration_since(self.tick).as_secs_f32().min(0.05);

		let remaining_time = self.deadline.saturating_duration_since(self.tick).as_secs_f32();
		self.tick = now;

		if crate::ui_motion::reduced() || now >= self.deadline {
			self.shown = self.ends.len() as f32;
		}
		let remaining = self.ends.len() as f32 - self.shown;
		// Recompute from backlog and time left, not a fixed typing rate. A burst
		// finishes within 120 ms; sustained fast output keeps the same short visual lag.
		let speed = 90_f32.max(remaining / remaining_time.max(0.001));

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

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use unicode_segmentation::UnicodeSegmentation;

	use super::{Instant, TextReveal};
	#[test]
	fn bursts_reveal_on_frames_without_splitting_graphemes_and_then_stop() {
		let now = Instant::now();
		let mut reveal = TextReveal::new(now);
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
		let mut reveal = TextReveal::new(now);

		reveal.sample("Original text", now);

		assert_eq!(reveal.sample("修正后的内容", now).0, "修正后的内容".len());
		assert_eq!(reveal.sample("", now), (0, false));
	}
	#[test]
	fn high_rate_output_keeps_a_short_lag_and_bursts_finish_on_time() {
		for per_frame in [10, 50] {
			let now = Instant::now();
			let mut reveal = TextReveal::new(now);
			let mut text = String::new();
			for frame in 0..180 {
				text.push_str(&"字".repeat(per_frame));
				let (end, _) = reveal.sample(&text, now + Duration::from_millis(frame * 16));
				let backlog = (text.len() - end) / "字".len();
				assert!(
					backlog <= per_frame * 9,
					"presentation must not fall behind fast transport"
				);
			}
			let last = now + Duration::from_millis(179 * 16);
			assert_eq!(
				reveal.sample(&text, last + Duration::from_millis(120)),
				(text.len(), false)
			);
		}
		let now = Instant::now();
		let mut reveal = TextReveal::new(now);
		let text = "字".repeat(20_000);
		assert!(reveal.sample(&text, now).1);
		let (middle, moving) = reveal.sample(&text, now + Duration::from_millis(60));
		assert!(moving && middle > 0 && middle < text.len());
		assert_eq!(reveal.sample(&text, now + Duration::from_millis(120)), (text.len(), false));
	}
}
