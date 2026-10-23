//! Bounded action rows move upward on arrival and settle to the latest action.
use super::super::activity::{MAX_RECENT, Operation};
use crate::ui_motion::Tween;
use std::time::{Duration, Instant};

pub(super) const LINE: f32 = crate::ui_theme::CAPTION_LINE_HEIGHT;
const HOLD: Duration = Duration::from_secs(6);
const FADE: Duration = Duration::from_millis(240);

struct Row {
	operation: Operation,
	observed: Instant,
	y: Tween,
	height: f32,
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
	pub current: bool,
	pub y: f32,
	pub height: f32,
	pub layout_height: f32,
	pub opacity: f32,
}

#[derive(Clone, Default)]
pub(super) struct Frame {
	pub lines: Vec<Line>,
	pub height: f32,
	pub layout_height: f32,
	pub moving: bool,
}

impl Frame {
	/// Keep one update at a distance; readable zoom reveals the recent rows.
	pub(super) fn latest(mut self) -> Self {
		self.lines.retain(|line| line.current);
		for line in &mut self.lines {
			line.y = 0.;
		}
		self.height = self.lines.first().map_or(0., |line| line.height);
		self.layout_height = self.lines.first().map_or(0., |line| line.layout_height);
		// Hidden rows do not keep requesting frames during their six-second hold.
		self.moving = self.lines.iter().any(|line| line.opacity < 1.);
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
		for operation in
			operations.iter().rev().take(MAX_RECENT).collect::<Vec<_>>().into_iter().rev()
		{
			if let Some(row) = self.rows.iter_mut().find(|row| row.operation.id == operation.id) {
				row.operation.text.clone_from(&operation.text);
			} else if !self.seen.contains(&operation.id) {
				self.seen.push(operation.id.clone());
				self.rows.push(Row {
					operation: operation.clone(),
					observed: now,
					y: Tween::new(self.rows.iter().map(|row| row.height).sum::<f32>() + 5.),
					height: LINE,
				});
				if self.rows.len() > MAX_RECENT {
					let mut oldest = self.rows.remove(0);
					oldest.y.target(-oldest.height, now);
					if self.leaving.is_none() {
						self.leaving = Some((oldest, now));
					}
				}
			}
		}
		if self.seen.len() > 12 {
			self.seen.drain(..self.seen.len() - 12);
		}
	}

	#[cfg(test)]
	pub fn frame(&mut self, now: Instant, reduced: bool) -> Frame {
		self.measured_frame(now, reduced, |_, _| (LINE, LINE))
	}

	pub fn measured_frame(
		&mut self,
		now: Instant,
		reduced: bool,
		mut measure: impl FnMut(&str, bool) -> (f32, f32),
	) -> Frame {
		if self.leaving.as_ref().is_some_and(|(_, started)| now.duration_since(*started) >= FADE) {
			self.leaving = None;
		}
		// Expire from the oldest end only. Fade and compact together; an old
		// snapshot cannot cause newer actions to disappear first.
		if self.leaving.is_none()
			&& self.rows.len() > 1
			&& now.duration_since(self.rows[0].observed) >= HOLD
		{
			let mut oldest = self.rows.remove(0);
			oldest.y.target(-oldest.height, now);
			self.leaving = Some((oldest, now));
		}
		let mut moving = false;
		let mut lines = Vec::new();
		let count = self.rows.len();
		let mut height = 0.;
		let mut layout_height = 0.;
		for (index, row) in self.rows.iter_mut().enumerate() {
			let age = now.duration_since(row.observed);
			let newest = index + 1 == count;
			let arrival =
				if reduced { 1. } else { (age.as_secs_f32() / FADE.as_secs_f32()).min(1.) };
			let opacity = arrival;
			let measured = measure(&row.operation.text, newest);
			row.height = measured.0;
			layout_height += measured.1;
			row.y.target(height, now);
			moving |= row.y.moving(now) || arrival < 1. || !newest;
			lines.push(Line {
				id: row.operation.id.clone(),
				text: row.operation.text.clone(),
				current: newest,
				y: if reduced { height } else { row.y.sample(now) },
				height: row.height,
				layout_height: measured.1,
				opacity,
			});
			height += row.height;
		}
		if let Some((row, started)) = &self.leaving {
			let progress = now.duration_since(*started).as_secs_f32() / FADE.as_secs_f32();
			if !reduced && progress < 1. {
				lines.push(Line {
					id: row.operation.id.clone(),
					text: row.operation.text.clone(),
					current: false,
					y: row.y.sample(now),
					height: row.height,
					layout_height: row.height,
					opacity: 1. - progress,
				});
				moving = true;
			} else {
				self.leaving = None;
			}
		}
		self.height.target(height, now);
		moving |= self.height.moving(now);
		Frame {
			lines,
			height: if reduced { height } else { self.height.sample(now) },
			layout_height,
			moving,
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
	fn wrapped_rows_use_their_actual_height_and_keep_the_last_result() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["1", "2", "3"]), now);
		let mut frame = stream.measured_frame(now, true, |_, current| {
			if current { (4. * LINE, 5. * LINE) } else { (2. * LINE, 3. * LINE) }
		});
		assert_eq!(
			frame.lines.iter().map(|line| line.y).collect::<Vec<_>>(),
			[0., 2. * LINE, 4. * LINE]
		);
		assert_eq!(frame.height, 8. * LINE);
		assert_eq!(frame.layout_height, 11. * LINE);
		assert_eq!(frame.clone().latest().height, 4. * LINE);
		for i in 0..4 {
			frame = stream.measured_frame(now + HOLD + FADE * i, true, |_, current| {
				if current { (4. * LINE, 5. * LINE) } else { (2. * LINE, 3. * LINE) }
			});
		}
		assert_eq!(frame.lines.len(), 1);
		assert_eq!(frame.lines[0].id, "3");
		assert_eq!(frame.height, 4. * LINE);
	}
	#[test]
	fn overflow_moves_the_oldest_up_and_keeps_five_actions() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["1", "2", "3", "4", "5"]), now);
		stream.sync("turn", &operations(&["2", "3", "4", "5", "6"]), now + FADE);
		let frame = stream.frame(now + FADE + FADE / 2, false);
		assert!(frame.lines.iter().any(|line| line.id == "1" && line.opacity < 1.));
		assert_eq!(frame.lines.iter().filter(|line| line.current).count(), 1);
		let frame = stream.frame(now + FADE * 3, false);
		assert_eq!(
			frame.lines.iter().map(|line| line.id.as_str()).collect::<Vec<_>>(),
			["2", "3", "4", "5", "6"]
		);
		let compact = frame.latest();
		assert_eq!(compact.lines.len(), 1);
		assert_eq!(compact.lines[0].id, "6");
		assert_eq!(compact.lines[0].y, 0.);
	}
	#[test]
	fn quiet_history_expires_oldest_first_then_compacts_to_one() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["1", "2", "3", "4", "5"]), now);
		let first = stream.frame(now + HOLD, false);
		assert_eq!(first.lines.last().unwrap().id, "1");
		let second = stream.frame(now + HOLD + FADE, false);
		assert_eq!(second.lines.last().unwrap().id, "2");
		for i in 2..7 {
			stream.frame(now + HOLD + FADE * i, false);
		}
		let final_frame = stream.frame(now + HOLD + FADE * 8, false);
		assert_eq!(final_frame.lines.len(), 1);
		assert_eq!(final_frame.lines[0].id, "5");
		assert_eq!(final_frame.lines[0].y, 0.);
		assert_eq!(final_frame.height, LINE);
	}
	#[test]
	fn old_snapshots_do_not_replay_and_updates_keep_the_latest_text() {
		let now = Instant::now();
		let mut stream = Stream::new("turn", &operations(&["a", "b", "c"]), now);
		for i in 0..6 {
			stream.frame(now + HOLD + FADE * i, false);
		}
		let mut updated = operations(&["a", "b", "c"]);
		updated[2].text = "Command finished".into();
		let later = now + HOLD + FADE * 7;
		stream.sync("turn", &updated, later);
		let frame = stream.frame(later + FADE, false);
		assert_eq!(frame.lines.len(), 1);
		assert_eq!(frame.lines[0].text, "Command finished");
		assert!(!frame.moving);
	}
	#[test]
	fn new_turn_and_reduced_motion_preserve_one_current_action() {
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
