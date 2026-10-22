//! Bounded action rows move upward on arrival and settle to the latest action.
use super::super::activity::Operation;
use crate::ui_motion::Tween;
use std::time::{Duration, Instant};

pub(super) const LINE: f32 = crate::ui_theme::CAPTION_LINE_HEIGHT;
const HOLD: Duration = Duration::from_secs(6);
const FADE: Duration = Duration::from_millis(240);

struct Row {
	operation: Operation,
	observed: Instant,
	y: Tween,
}

pub(super) struct Stream {
	scope: String,
	seen: Vec<String>,
	rows: Vec<Row>,
	leaving: Option<(Row, Instant)>,
	height: Tween,
}

#[derive(Clone)]
pub(super) struct Line {
	pub id: String,
	pub text: String,
	pub y: f32,
	pub opacity: f32,
}

#[derive(Clone, Default)]
pub(super) struct Frame {
	pub lines: Vec<Line>,
	pub height: f32,
	pub moving: bool,
	latest: Option<String>,
}

impl Frame {
	/// Keep the current action in the overview; selection reveals the recent rows.
	pub(super) fn latest(mut self) -> Self {
		self.lines.retain(|line| Some(&line.id) == self.latest.as_ref());
		for line in &mut self.lines {
			line.y = 0.;
		}
		self.height = if self.lines.is_empty() { 0. } else { LINE };
		self
	}
}

impl Stream {
	pub fn new(scope: &str, operations: &[Operation], now: Instant) -> Self {
		let mut stream = Self {
			scope: scope.into(),
			seen: vec![],
			rows: vec![],
			leaving: None,
			height: Tween::new(0.),
		};
		stream.sync(scope, operations, now);
		for (index, row) in stream.rows.iter_mut().enumerate() {
			row.y = Tween::new(index as f32 * LINE);
			row.observed = now - FADE;
		}
		stream.height = Tween::new(stream.rows.len() as f32 * LINE);
		stream
	}

	pub fn sync(&mut self, scope: &str, operations: &[Operation], now: Instant) {
		if self.scope != scope {
			*self = Self::new(scope, operations, now);
			return;
		}
		for operation in operations.iter().rev().take(3).collect::<Vec<_>>().into_iter().rev() {
			if let Some(row) = self.rows.iter_mut().find(|row| row.operation.id == operation.id) {
				row.operation.text.clone_from(&operation.text);
			} else if !self.seen.contains(&operation.id) {
				self.seen.push(operation.id.clone());
				self.rows.push(Row {
					operation: operation.clone(),
					observed: now,
					y: Tween::new(self.rows.len() as f32 * LINE + 5.),
				});
				if self.rows.len() > 3 {
					let mut oldest = self.rows.remove(0);
					oldest.y.target(-LINE, now);
					self.leaving = Some((oldest, now));
				}
			}
		}
		if self.seen.len() > 12 {
			self.seen.drain(..self.seen.len() - 12);
		}
	}

	pub fn frame(&mut self, now: Instant, reduced: bool) -> Frame {
		let latest = self.rows.last().map(|row| row.operation.id.clone());
		self.rows.retain(|row| {
			Some(&row.operation.id) == latest.as_ref()
				|| now.duration_since(row.observed) < HOLD + FADE
		});
		let mut moving = false;
		let mut lines = Vec::new();
		let count = self.rows.len();
		for (index, row) in self.rows.iter_mut().enumerate() {
			let age = now.duration_since(row.observed);
			let newest = index + 1 == count;
			let arrival =
				if reduced { 1. } else { (age.as_secs_f32() / FADE.as_secs_f32()).min(1.) };
			let fading = !newest && age >= HOLD;
			let opacity = if fading {
				if reduced {
					0.
				} else {
					1. - ((age - HOLD).as_secs_f32() / FADE.as_secs_f32()).min(1.)
				}
			} else {
				arrival
			};
			row.y.target(index as f32 * LINE, now);
			moving |= row.y.moving(now) || arrival < 1. || !newest;
			lines.push(Line {
				id: row.operation.id.clone(),
				text: row.operation.text.clone(),
				y: if reduced { index as f32 * LINE } else { row.y.sample(now) },
				opacity,
			});
		}
		if let Some((row, started)) = &self.leaving {
			let progress = now.duration_since(*started).as_secs_f32() / FADE.as_secs_f32();
			if !reduced && progress < 1. {
				lines.push(Line {
					id: row.operation.id.clone(),
					text: row.operation.text.clone(),
					y: row.y.sample(now),
					opacity: 1. - progress,
				});
				moving = true;
			} else {
				self.leaving = None;
			}
		}
		self.height.target(self.rows.len() as f32 * LINE, now);
		moving |= self.height.moving(now);
		Frame {
			lines,
			height: if reduced { self.rows.len() as f32 * LINE } else { self.height.sample(now) },
			moving,
			latest,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn operations(ids: &[&str]) -> Vec<Operation> {
		ids.iter().map(|id| Operation { id: (*id).into(), text: format!("Run {id}") }).collect()
	}
	#[test]
	fn compact_view_keeps_the_latest_arrival_and_preserves_recent_actions() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["1", "2", "3"]), now);
		stream.sync("turn", &operations(&["2", "3", "4"]), now + FADE);
		let frame = stream.frame(now + FADE + FADE / 2, false);
		assert!(frame.lines.iter().any(|line| line.id == "1"), "old row is still leaving");
		let compact = frame.latest();
		assert_eq!(compact.lines.len(), 1);
		assert_eq!(compact.lines[0].id, "4");
		assert_eq!(compact.lines[0].y, 0.);
		assert_eq!(compact.height, LINE);
		let expanded = stream.frame(now + FADE * 3, false);
		assert_eq!(
			expanded.lines.iter().map(|line| line.id.as_str()).collect::<Vec<_>>(),
			["2", "3", "4"]
		);
	}
	#[test]
	fn repeated_reads_and_streaming_updates_do_not_replay_old_actions() {
		let now = Instant::now();
		let mut stream = Stream::new("thread:turn", &operations(&["a", "b", "c"]), now);
		let later = now + HOLD + FADE;
		assert_eq!(stream.frame(later, false).lines.len(), 1);
		let mut updated = operations(&["a", "b", "c"]);
		updated[2].text = "Command finished".into();
		stream.sync("thread:turn", &updated, later);
		let frame = stream.frame(later + FADE, false);
		assert_eq!(frame.lines.len(), 1);
		assert_eq!(frame.lines[0].text, "Command finished");
		assert!(!frame.moving);
	}
	#[test]
	fn fourth_action_scrolls_up_and_idle_keeps_one_without_changing_source() {
		let now = Instant::now();
		let mut stream = Stream::new("a", &operations(&["1", "2", "3"]), now);
		stream.sync("a", &operations(&["2", "3", "4"]), now + FADE);
		assert_eq!(stream.rows.len(), 3);
		assert!(stream.frame(now + FADE, false).moving);
		let frame = stream.frame(now + FADE * 3, false);
		assert_eq!(
			frame.lines.iter().map(|line| line.id.as_str()).collect::<Vec<_>>(),
			["2", "3", "4"]
		);
		let frame = stream.frame(now + HOLD + FADE * 3, false);
		assert_eq!(frame.lines.len(), 1);
		assert_eq!(frame.lines[0].id, "4");
	}
	#[test]
	fn new_turn_clears_old_actions_and_reduced_motion_has_no_transition() {
		let now = Instant::now();
		let mut stream = Stream::new("old", &operations(&["1", "2"]), now);
		stream.sync("new", &operations(&["next"]), now);
		let frame = stream.frame(now, true);
		assert_eq!(frame.lines.len(), 1);
		assert_eq!(frame.lines[0].text, "Run next");
		assert_eq!(frame.lines[0].opacity, 1.);
		assert_eq!(frame.height, LINE);
	}
}
