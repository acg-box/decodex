//! Shared grapheme-safe presentation for model output, live captions and dictation.
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation as _;

pub(crate) struct TextReveal {
	text: String,
	ends: Vec<usize>,
	shown: f32,
	tick: Instant,
	deadline: Instant,
	pacing: Option<OutputPacing>,
}
impl TextReveal {
	pub(crate) fn new(now: Instant) -> Self {
		Self {
			text: String::new(),
			ends: Vec::new(),
			shown: 0.,
			tick: now,
			deadline: now,
			pacing: None,
		}
	}

	pub(crate) fn output(text: &str, complete: bool, now: Instant) -> Self {
		let mut state = if complete { Self::settled(text, now) } else { Self::new(now) };
		state.pacing = Some(OutputPacing::default());
		state
	}

	pub(crate) fn finish(&mut self, now: Instant) {
		self.deadline = self.deadline.min(now + Duration::from_millis(160));
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
			let previous_count = self.ends.len();
			if !text.starts_with(&self.text) {
				// Provider corrections replace the visible prefix without replaying it.
				self.shown = text.graphemes(true).count() as f32;
			}

			self.text = text.into();
			self.ends = text.grapheme_indices(true).map(|(i, g)| i + g.len()).collect();
			let horizon = self.pacing.as_mut().map_or(0.12, |pacing| {
				pacing.receive(self.ends.len().saturating_sub(previous_count), now);
				(pacing.buffer() * 1.5).clamp(0.3, 0.7)
			});
			self.deadline = now + Duration::from_secs_f32(horizon);
		}

		let elapsed = now.saturating_duration_since(self.tick).as_secs_f32().min(0.05);

		let remaining_time = self.deadline.saturating_duration_since(self.tick).as_secs_f32();
		self.tick = now;

		if crate::ui_motion::reduced() || now >= self.deadline {
			self.shown = self.ends.len() as f32;
		}
		let remaining = self.ends.len() as f32 - self.shown;
		// Output keeps a small adaptive reserve across network gaps. Captions and
		// dictation retain their short 120 ms response time.
		let speed = if let Some(pacing) = &mut self.pacing {
			pacing.speed(remaining, elapsed).max(remaining / remaining_time.max(0.001))
		} else {
			90_f32.max(remaining / remaining_time.max(0.001))
		};

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

/// Estimate only arrival cadence and throughput; transport and saved text stay untouched.
struct OutputPacing {
	arrival: Option<Instant>,
	interval: f32,
	jitter: f32,
	rate: f32,
	speed: f32,
}
impl Default for OutputPacing {
	fn default() -> Self {
		Self { arrival: None, interval: 0.16, jitter: 0.04, rate: 0., speed: 0. }
	}
}
impl OutputPacing {
	fn receive(&mut self, count: usize, now: Instant) {
		if count == 0 {
			return;
		}
		if let Some(previous) = self.arrival {
			let gap = now.saturating_duration_since(previous).as_secs_f32().clamp(0.008, 1.5);
			self.jitter += ((gap - self.interval).abs() - self.jitter) * 0.25;
			self.interval += (gap - self.interval) * 0.25;
			let weight = (1. - (-gap / 0.8).exp()).clamp(0.08, 0.5);
			self.rate += (count as f32 / gap - self.rate) * weight;
		} else {
			self.rate = count as f32 / self.buffer();
			self.speed = self.rate;
		}
		self.arrival = Some(now);
	}

	fn buffer(&self) -> f32 {
		(self.interval + 2. * self.jitter).clamp(0.18, 0.5)
	}

	fn speed(&mut self, remaining: f32, elapsed: f32) -> f32 {
		let reserve = self.rate * self.buffer();
		let desired = (self.rate + (remaining - reserve) / 0.3).max(8.);
		self.speed += (desired - self.speed) * (1. - (-elapsed / 0.1).exp());
		self.speed
	}
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use unicode_segmentation::UnicodeSegmentation;

	use super::{Instant, TextReveal};
	#[test]
	fn output_bridges_irregular_arrivals_and_adapts_to_a_faster_stream() {
		for frame_ms in [6, 16] {
			let now = Instant::now();
			let mut reveal = TextReveal::output("", false, now);
			let gaps = [120, 300, 80, 240, 360, 180];
			let mut next = 0;
			let mut packet = 0;
			let mut text = String::new();
			let mut previous = 0;
			let mut last_progress = 0;
			let mut longest_pause = 0;
			for ms in (0..6000).step_by(frame_ms) {
				if ms >= next {
					let gap = gaps[packet % gaps.len()];
					let rate = if ms < 3000 { 100 } else { 400 };
					text.push_str(&"a".repeat(gap * rate / 1000));
					next += gap;
					packet += 1;
				}
				let (end, _) = reveal.sample(&text, now + Duration::from_millis(ms as u64));
				assert!(end >= previous && end <= text.len());
				if end != previous {
					last_progress = ms;
				}
				if ms > 1000 {
					longest_pause = longest_pause.max(ms - last_progress);
				}
				assert!(text.len() - end <= 280, "the playback buffer must stay bounded");
				previous = end;
			}
			assert!(
				longest_pause < 150,
				"regular network gaps must not become long visual pauses: {longest_pause} ms"
			);
			let finished = now + Duration::from_millis(6000);
			reveal.finish(finished);
			assert_eq!(
				reveal.sample(&text, finished + Duration::from_millis(160)),
				(text.len(), false)
			);
		}
	}

	#[test]
	fn output_stalls_finish_within_a_bound_and_history_does_not_replay() {
		let now = Instant::now();
		let text = "你好👨‍👩‍👧‍👦e\u{301}".repeat(200);
		let mut reveal = TextReveal::output(&text, false, now);
		assert!(reveal.sample(&text, now).1);
		let (end, moving) = reveal.sample(&text, now + Duration::from_millis(120));
		assert!(moving, "output must retain a reserve beyond the old 120 ms catch-up");
		assert!(text.grapheme_indices(true).any(|(i, _)| i == end));
		assert_eq!(reveal.sample(&text, now + Duration::from_millis(700)), (text.len(), false));
		let mut history = TextReveal::output(&text, true, now);
		assert_eq!(history.sample(&text, now), (text.len(), false));
	}

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
	fn promoted_output_keeps_its_prefix_and_animates_only_new_text() {
		let now = Instant::now();
		let prefix = "An existing paragraph.\n\n";
		let mut reveal = TextReveal::settled(prefix, now);
		assert_eq!(reveal.sample(prefix, now), (prefix.len(), false));
		let text = format!("{prefix}{}", "New text. ".repeat(12));
		let (first, moving) = reveal.sample(&text, now);
		assert!(moving && first >= prefix.len() && first < text.len());
		assert_eq!(reveal.sample(&text, now + Duration::from_millis(120)), (text.len(), false));
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
