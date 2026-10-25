//! One current message per agent. Replacements fade through without replaying history.
use std::time::{Duration, Instant};

use crate::{
	shell::agent_surface::work_board::activity::{MAX_RECENT, Operation, OperationKind},
	ui_theme::CAPTION_LINE_HEIGHT,
};

pub(super) const LINE: f32 = CAPTION_LINE_HEIGHT;

const FADE: Duration = Duration::from_millis(280);
const RESIZE: Duration = Duration::from_millis(140);

pub(super) struct Stream {
	scope: String,
	seen: Vec<String>,
	current: Option<Operation>,
	previous: Option<Operation>,
	changed: Instant,
	height: Option<Extent>,
	arriving: bool,
	measured_text: Option<String>,
}
impl Stream {
	pub fn new(scope: &str, operations: &[Operation], now: Instant) -> Self {
		Self {
			scope: scope.into(),
			seen: operations.iter().map(|op| op.id.clone()).collect(),
			current: operations.last().cloned(),
			previous: None,
			changed: now - FADE,
			height: None,
			arriving: false,
			measured_text: None,
		}
	}

	pub fn sync(&mut self, scope: &str, operations: &[Operation], now: Instant) {
		if self.scope != scope {
			let height = self.height.take();

			*self = Self::new(scope, operations, now);
			self.arriving = height.is_some() && self.current.is_some();
			self.height = height;
			self.changed = now;

			return;
		}

		let Some(next) = operations.last() else {
			if self.current.as_ref().is_some_and(|current| current.kind == OperationKind::Question)
			{
				self.current = None;
				self.previous = None;
				self.height = None;
				self.arriving = false;
			}

			return;
		};

		if self.current.as_ref().is_some_and(|current| current.id == next.id) {
			// A streaming delta or final phase updates the same bubble in place.
			self.current = Some(next.clone());
		} else if !self.seen.contains(&next.id)
			|| next.kind == OperationKind::Question
			|| self.current.as_ref().is_some_and(|current| current.kind == OperationKind::Question)
		{
			if self.current.is_none() {
				self.height = Some(Extent::fixed(0., now));
			}

			self.previous = self.current.replace(next.clone());
			self.changed = now;
			self.arriving = true;
		}

		for op in operations {
			if !self.seen.contains(&op.id) {
				self.seen.push(op.id.clone());
			}
		}

		if self.seen.len() > MAX_RECENT * 2 {
			self.seen.drain(..self.seen.len() - MAX_RECENT * 2);
		}
	}

	pub fn measured_frame(
		&mut self,
		now: Instant,
		reduced: bool,
		mut measure: impl FnMut(&str) -> (f32, f32),
	) -> Frame {
		let Some(current) = &self.current else {
			let Some(height) = &mut self.height else { return Frame::default() };

			height.target(0., now, now);

			if reduced {
				*height = Extent::fixed(0., now);
			}

			return Frame {
				height: height.sample(now),
				moving: height.moving(now),
				..Frame::default()
			};
		};
		let measured = measure(&current.text);
		let height = self.height.get_or_insert_with(|| Extent::fixed(measured.0, now));
		let leave = if self.previous.is_some() { FADE / 4 } else { Duration::ZERO };

		if self.measured_text.as_deref() == Some(&current.text) && height.to != measured.0 {
			// Zoom and display-density changes already animate the camera. Only
			// new source text starts a second, independent speech-size transition.
			*height = Extent::fixed(measured.0, now);
		} else {
			height.target(measured.0, now, self.changed + leave);
		}

		self.measured_text = Some(current.text.clone());

		if reduced {
			*height = Extent::fixed(measured.0, now);
		}

		// Old text leaves before resizing. New text enters after the resize, so
		// changing line counts cannot stack glyphs or squeeze readable text.
		let enter = self.changed + leave + if leave.is_zero() { Duration::ZERO } else { RESIZE };
		let enter = if height.from != height.to { enter.max(height.ends_at()) } else { enter };
		let opacity = if self.arriving && !reduced {
			(now.saturating_duration_since(enter).as_secs_f32() / (FADE / 4).as_secs_f32()).min(1.)
		} else {
			1.
		};

		if opacity >= 1. {
			self.previous = None;
			self.arriving = false;
		}

		let mut frame = Frame {
			height: height.sample(now),
			layout_height: measured.1,
			moving: self.arriving || height.moving(now),
			lines: vec![Line {
				id: current.id.clone(),
				text: current.text.clone(),
				kind: current.kind,
				y: 0.,
				opacity,
			}],
		};

		if let Some(previous) = &self.previous {
			let measured = measure(&previous.text);

			frame.layout_height = frame.layout_height.max(measured.1);

			frame.lines.insert(
				0,
				Line {
					id: previous.id.clone(),
					text: previous.text.clone(),
					kind: previous.kind,
					y: 0.,
					opacity: (1.
						- now.saturating_duration_since(self.changed).as_secs_f32()
							/ (FADE / 4).as_secs_f32())
					.max(0.),
				},
			);
		}

		frame
	}
}

#[derive(Clone)]
pub(super) struct Line {
	pub id: String,
	pub text: String,
	pub kind: OperationKind,
	pub y: f32,
	pub opacity: f32,
}

#[derive(Clone, Default)]
pub(super) struct Frame {
	pub lines: Vec<Line>,
	pub height: f32,
	pub layout_height: f32,
	pub moving: bool,
}

struct Extent {
	from: f32,
	to: f32,
	started: Instant,
}
impl Extent {
	fn fixed(value: f32, now: Instant) -> Self {
		Self { from: value, to: value, started: now }
	}

	fn sample(&self, now: Instant) -> f32 {
		let t = (now.saturating_duration_since(self.started).as_secs_f32() / RESIZE.as_secs_f32())
			.min(1.);

		self.from + (self.to - self.from) * (t * t * (3. - 2. * t))
	}

	fn target(&mut self, value: f32, now: Instant, not_before: Instant) {
		if self.to != value {
			self.from = self.sample(now);
			self.to = value;
			self.started = now.max(not_before);
		}
	}

	fn ends_at(&self) -> Instant {
		if self.from == self.to { self.started } else { self.started + RESIZE }
	}

	fn moving(&self, now: Instant) -> bool {
		self.from != self.to && now.saturating_duration_since(self.started) < RESIZE
	}
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::work_board::relations::stream::{
		Duration, FADE, Frame, Instant, LINE, Operation, OperationKind, RESIZE, Stream,
	};
	fn operations(ids: &[&str]) -> Vec<Operation> {
		ids.iter()
			.map(|id| Operation {
				id: (*id).into(),
				text: format!("Run {id}"),
				kind: OperationKind::Action,
			})
			.collect()
	}
	fn frame(stream: &mut Stream, now: Instant, reduced: bool) -> Frame {
		stream.measured_frame(now, reduced, |_| (LINE, LINE))
	}

	#[test]
	fn restored_history_shows_only_the_latest_message() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["1", "2", "3", "4", "5"]), now);
		let result = frame(&mut stream, now, false);

		assert_eq!(result.lines.len(), 1);
		assert_eq!(result.lines[0].id, "5");
		assert_eq!(result.lines[0].opacity, 1.);
		assert!(!result.moving);
	}

	#[test]
	fn replacement_never_overprints_messages_and_then_keeps_only_one() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["1"]), now);

		stream.sync("turn", &operations(&["1", "2"]), now);

		for elapsed in [FADE / 4, FADE / 2, FADE * 3 / 4] {
			let result = frame(&mut stream, now + elapsed, false);

			assert_eq!(result.lines.len(), 2);
			assert_eq!(result.height, LINE, "messages share one slot instead of forming a log");
			assert!(result.lines.iter().filter(|line| line.opacity > 0.).count() <= 1);
		}

		assert!(frame(&mut stream, now + FADE / 8, false).lines[0].opacity > 0.);
		assert!(frame(&mut stream, now + FADE * 7 / 8, false).lines[1].opacity > 0.);

		let result = frame(&mut stream, now + FADE, false);

		assert_eq!(result.lines.len(), 1);
		assert_eq!(result.lines[0].id, "2");
		assert!(!result.moving);
		assert_eq!(frame(&mut stream, now + Duration::from_secs(600), false).lines[0].id, "2");
	}

	#[test]
	fn old_snapshots_do_not_replay_and_deltas_do_not_restart_the_fade() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["a", "b", "c"]), now);

		stream.sync("turn", &operations(&["a", "b"]), now);

		assert_eq!(frame(&mut stream, now, false).lines[0].id, "c");

		let mut update = operations(&["c"]);

		update[0].text = "Finished the check".into();
		update[0].kind = OperationKind::Result;

		stream.sync("turn", &update, now + FADE);

		let result = frame(&mut stream, now + FADE, false);

		assert_eq!(result.lines[0].text, "Finished the check");
		assert_eq!(result.lines[0].kind, OperationKind::Result);
		assert_eq!(result.lines[0].opacity, 1.);
		assert!(!result.moving);
	}

	#[test]
	fn new_turn_clears_old_content_and_reduced_motion_finishes_the_replacement() {
		let now = Instant::now();
		let mut stream = Stream::new("old", &operations(&["1", "2"]), now);

		stream.sync("new", &[], now);

		assert!(frame(&mut stream, now, false).lines.is_empty());

		stream.sync("new", &operations(&["next"]), now);

		let result = frame(&mut stream, now, true);

		assert_eq!(result.lines.len(), 1);
		assert_eq!(result.lines[0].opacity, 1.);
		assert!(!result.moving);
	}

	#[test]
	fn questions_stay_until_withdrawn_and_then_restore_the_last_native_message() {
		let now = Instant::now();
		let original = operations(&["a"]);
		let question = vec![Operation {
			id: "question".into(),
			text: "Which profile?".into(),
			kind: OperationKind::Question,
		}];
		let mut stream = Stream::new("turn", &original, now);

		stream.sync("turn", &question, now);

		assert_eq!(
			frame(&mut stream, now + Duration::from_secs(3_600), false).lines[0].kind,
			OperationKind::Question
		);

		stream.sync("turn", &original, now + Duration::from_secs(3_601));

		assert_eq!(frame(&mut stream, now + Duration::from_secs(3_601), true).lines[0].id, "a");

		stream.sync("turn", &question, now + Duration::from_secs(3_602));
		stream.sync("turn", &[], now + Duration::from_secs(3_603));

		assert!(frame(&mut stream, now + Duration::from_secs(3_603), false).lines.is_empty());
	}

	#[test]
	fn first_arrival_requests_frames_even_without_an_outgoing_message() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &[], now);

		stream.sync("turn", &operations(&["a"]), now);

		assert!(frame(&mut stream, now, false).moving);
		assert_eq!(frame(&mut stream, now + FADE, false).lines[0].opacity, 1.);
	}

	#[test]
	fn replacements_resize_before_revealing_longer_or_shorter_text() {
		let now = Instant::now();
		let measure = |text: &str| {
			let height = if text.ends_with("long") { 5. * LINE } else { LINE };

			(height, height)
		};
		let mut stream = Stream::new("turn", &operations(&["short"]), now);

		stream.measured_frame(now, false, measure);

		for (index, id, from, to) in
			[(0, "long", LINE, 5. * LINE), (1, "short-again", 5. * LINE, LINE)]
		{
			let start = now + Duration::from_secs(index);

			stream.sync("turn", &operations(&[id]), start);

			assert_eq!(stream.measured_frame(start, false, measure).height, from);

			for ms in [35, 70, 105, 140, 175, 210, 245, 280] {
				let result =
					stream.measured_frame(start + Duration::from_millis(ms), false, measure);

				assert!((from.min(to)..=from.max(to)).contains(&result.height));
				assert!(result.lines.iter().filter(|line| line.opacity > 0.).count() <= 1);

				if result.lines.last().unwrap().opacity > 0. {
					assert_eq!(result.height, to, "incoming text waits for its final bounds");
				}
			}

			assert_eq!(stream.measured_frame(start + FADE, false, measure).height, to);
		}
	}

	#[test]
	fn late_paint_and_streaming_growth_do_not_jump_or_reveal_clipped_text() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["a"]), now);

		stream.measured_frame(now, false, |_| (LINE, LINE));
		stream.sync("turn", &operations(&["b"]), now);

		let late = now + FADE;
		let result = stream.measured_frame(late, false, |_| (4. * LINE, 4. * LINE));

		assert_eq!(result.height, LINE);
		assert!(result.lines.iter().all(|line| line.opacity == 0.));

		let settled = late + RESIZE + FADE / 4;

		assert_eq!(
			stream.measured_frame(settled, false, |_| (4. * LINE, 4. * LINE)).lines[0].opacity,
			1.
		);

		let mut delta = operations(&["b"]);

		delta[0].text = "An expanded streaming sentence".into();

		stream.sync("turn", &delta, settled);

		assert_eq!(
			stream.measured_frame(settled, false, |_| (6. * LINE, 6. * LINE)).height,
			4. * LINE
		);

		let halfway = settled + RESIZE / 2;
		let before = stream.measured_frame(halfway, false, |_| (6. * LINE, 6. * LINE)).height;

		delta[0].text = "A revised sentence".into();

		stream.sync("turn", &delta, halfway);

		let retargeted = stream.measured_frame(halfway, false, |_| (3. * LINE, 3. * LINE));

		assert_eq!(retargeted.height, before, "a new delta starts from the current painted size");
		assert_eq!(retargeted.lines.len(), 1);
		assert_eq!(retargeted.lines[0].opacity, 1.);
		assert_eq!(
			stream.measured_frame(halfway, true, |_| (3. * LINE, 3. * LINE)).height,
			3. * LINE
		);
	}

	#[test]
	fn new_turn_with_no_output_collapses_without_replaying_old_text() {
		let now = Instant::now();
		let mut stream = Stream::new("old", &operations(&["a"]), now);

		stream.measured_frame(now, false, |_| (4. * LINE, 4. * LINE));
		stream.sync("new", &[], now);

		let first = frame(&mut stream, now, false);

		assert_eq!(first.height, 4. * LINE);
		assert!(first.lines.is_empty() && first.moving);

		let last = frame(&mut stream, now + RESIZE, false);

		assert_eq!(last.height, 0.);
		assert!(last.lines.is_empty() && !last.moving);
	}
}
