//! One current message per agent. Replacements fade through without replaying history.
use super::super::activity::{MAX_RECENT, Operation, OperationKind};
use std::time::{Duration, Instant};

pub(super) const LINE: f32 = crate::ui_theme::CAPTION_LINE_HEIGHT;
const FADE: Duration = Duration::from_millis(200);

pub(super) struct Stream {
	scope: String,
	seen: Vec<String>,
	current: Option<Operation>,
	previous: Option<Operation>,
	changed: Instant,
}

#[derive(Clone)]
pub(super) struct Line {
	pub id: String,
	pub text: String,
	pub kind: OperationKind,
	pub y: f32,
	pub height: f32,
	pub opacity: f32,
}

#[derive(Clone, Default)]
pub(super) struct Frame {
	pub lines: Vec<Line>,
	pub height: f32,
	pub layout_height: f32,
	pub moving: bool,
}

impl Stream {
	pub fn new(scope: &str, operations: &[Operation], now: Instant) -> Self {
		Self {
			scope: scope.into(),
			seen: operations.iter().map(|op| op.id.clone()).collect(),
			current: operations.last().cloned(),
			previous: None,
			changed: now - FADE,
		}
	}

	pub fn sync(&mut self, scope: &str, operations: &[Operation], now: Instant) {
		if self.scope != scope {
			*self = Self::new(scope, operations, now);
			return;
		}
		let Some(next) = operations.last() else {
			if self.current.as_ref().is_some_and(|current| current.kind == OperationKind::Question)
			{
				self.current = None;
				self.previous = None;
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
			self.previous = self.current.replace(next.clone());
			self.changed = now;
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
		let progress = if reduced {
			1.
		} else {
			(now.duration_since(self.changed).as_secs_f32() / FADE.as_secs_f32()).min(1.)
		};
		if progress >= 1. {
			self.previous = None;
		}
		let Some(current) = &self.current else { return Frame::default() };
		let measured = measure(&current.text);
		// Two readable messages in the same slot produce overlapping glyphs.
		// Retire the old text before revealing the replacement; keep its footprint
		// until the transition ends so the agent and its neighbours stay still.
		let opacity = if self.previous.is_some() { (progress * 2. - 1.).max(0.) } else { progress };
		let mut frame = Frame {
			height: measured.0,
			layout_height: measured.1,
			moving: progress < 1.,
			lines: vec![Line {
				id: current.id.clone(),
				text: current.text.clone(),
				kind: current.kind,
				y: (1. - progress) * 3.,
				height: measured.0,
				opacity,
			}],
		};
		if let Some(previous) = &self.previous {
			let measured = measure(&previous.text);
			frame.height = frame.height.max(measured.0);
			frame.layout_height = frame.layout_height.max(measured.1);
			frame.lines.insert(
				0,
				Line {
					id: previous.id.clone(),
					text: previous.text.clone(),
					kind: previous.kind,
					y: -progress * 3.,
					height: measured.0,
					opacity: (1. - progress * 2.).max(0.),
				},
			);
		}
		frame
	}
}

#[cfg(test)]
mod tests {
	use super::*;
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
		assert!(frame(&mut stream, now + FADE / 4, false).lines[0].opacity > 0.);
		assert!(frame(&mut stream, now + FADE * 3 / 4, false).lines[1].opacity > 0.);
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
			frame(&mut stream, now + Duration::from_secs(3600), false).lines[0].kind,
			OperationKind::Question
		);
		stream.sync("turn", &original, now + Duration::from_secs(3601));
		assert_eq!(frame(&mut stream, now + Duration::from_secs(3601), true).lines[0].id, "a");
		stream.sync("turn", &question, now + Duration::from_secs(3602));
		stream.sync("turn", &[], now + Duration::from_secs(3603));
		assert!(frame(&mut stream, now + Duration::from_secs(3603), false).lines.is_empty());
	}

	#[test]
	fn first_arrival_requests_frames_even_without_an_outgoing_message() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &[], now);
		stream.sync("turn", &operations(&["a"]), now);
		assert!(frame(&mut stream, now, false).moving);
		assert_eq!(frame(&mut stream, now + FADE, false).lines[0].opacity, 1.);
	}
}
