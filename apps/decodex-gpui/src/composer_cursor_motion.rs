//! One insertion cursor, with the four-corner spring used by Zed's cursor_animation.
// Reference: zed-industries/zed e462f873c622f6265736bf8468e7a017f4ba99de.
// Spring physics and corner timing adapted from vscode-neovide-cursor (MIT).
// Copyright (c) 2025 LengineerC. See assets/licenses/vscode-neovide-cursor.txt.
use std::time::{Duration, Instant};

use gpui::{Bounds, Pixels, Point, point, px};

use super::Shape;

const TRAIL_FACTORS: [f32; 4] = [1., 0.9, 0.5, 0.3];

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Viewport {
	pub bounds: Bounds<Pixels>,
	pub scroll: Pixels,
	pub scale: f32,
	pub shape: Shape,
}

#[derive(Clone, Copy, Default)]
struct Corner {
	position: Point<f32>,
	velocity: Point<f32>,
	seconds: f32,
}

struct Flight {
	corners: [Corner; 4],
	target: Bounds<Pixels>,
	logical: usize,
	viewport: Viewport,
	frame_at: Instant,
	moving: bool,
}

#[derive(Default)]
pub(super) struct Motion(Option<Flight>);

impl Motion {
	pub fn reset(&mut self) {
		self.0 = None;
	}

	pub fn sample(
		&mut self,
		logical: usize,
		target: Bounds<Pixels>,
		viewport: Viewport,
		now: Instant,
	) -> Option<[Point<Pixels>; 4]> {
		let destinations = corners(target);
		let snap = self.0.as_ref().is_none_or(|state| {
			state.viewport != viewport
				|| (state.logical == logical && state.target != target)
				|| (state.moving && now.duration_since(state.frame_at) > Duration::from_millis(250))
		});
		if snap {
			self.0 = Some(Flight {
				corners: destinations.map(|position| Corner { position, ..Default::default() }),
				target,
				logical,
				viewport,
				frame_at: now,
				moving: false,
			});
			return None;
		}
		let state = self.0.as_mut()?;
		let elapsed = if state.moving {
			now.duration_since(state.frame_at).as_secs_f32().min(1. / 30.)
		} else {
			0.
		};
		if state.logical != logical && state.target != target {
			let delta = target.origin - state.target.origin;
			let jump = point(delta.x / target.size.width, delta.y / target.size.height);
			let base = if jump.x.abs() <= 8. && jump.y.abs() < 0.001 { 0.05 } else { 0.125 };
			let mut order: [(usize, f32); 4] = std::array::from_fn(|i| {
				(i, alignment(destinations[i] - state.corners[i].position, i))
			});
			order.sort_by(|a, b| a.1.total_cmp(&b.1));
			for (rank, (i, _)) in order.into_iter().enumerate() {
				let corner = &mut state.corners[i];
				corner.seconds =
					if alignment(jump, i) > 0.5 { 0.02 } else { base * TRAIL_FACTORS[rank] };
				if corner.seconds > 0.075 {
					corner.velocity = Point::default();
				}
			}
			state.moving = true;
		}
		state.target = target;
		state.logical = logical;
		state.frame_at = now;
		if !state.moving {
			return None;
		}
		state.moving = false;
		for (corner, destination) in state.corners.iter_mut().zip(destinations) {
			if elapsed >= corner.seconds {
				corner.position = destination;
				corner.velocity = Point::default();
			} else if elapsed > 0. {
				// Closed-form critically damped spring; independent of the display rate.
				let omega = 4. / corner.seconds;
				let error = destination - corner.position;
				let combined = error * omega + corner.velocity;
				let decay = (-omega * elapsed).exp();
				corner.position = destination - (error + combined * elapsed) * decay;
				corner.velocity = (corner.velocity - combined * (omega * elapsed)) * decay;
			}
			let error = destination - corner.position;
			state.moving |= error.x.abs().max(error.y.abs()) > 0.25;
		}
		if !state.moving {
			for (corner, position) in state.corners.iter_mut().zip(destinations) {
				corner.position = position;
				corner.velocity = Point::default();
			}
			return None;
		}
		Some(state.corners.map(|c| point(px(c.position.x), px(c.position.y))))
	}
}

fn corners(bounds: Bounds<Pixels>) -> [Point<f32>; 4] {
	[
		point(bounds.left().into(), bounds.top().into()),
		point(bounds.right().into(), bounds.top().into()),
		point(bounds.right().into(), bounds.bottom().into()),
		point(bounds.left().into(), bounds.bottom().into()),
	]
}

fn alignment(delta: Point<f32>, index: usize) -> f32 {
	let direction = [point(-1., -1.), point(1., -1.), point(1., 1.), point(-1., 1.)][index];
	let length = delta.x.hypot(delta.y);
	if length < 0.001 {
		0.
	} else {
		(delta.x * direction.x + delta.y * direction.y) / (length * 2_f32.sqrt())
	}
}

#[cfg(test)]
mod tests {
	use super::{
		Bounds, Duration, Instant, Motion, Pixels, Point, Shape, Viewport, corners, point, px,
	};

	fn bounds(x: f32, y: f32) -> Bounds<Pixels> {
		Bounds::new(point(px(x), px(y)), gpui::size(px(1.5), px(17.)))
	}

	fn viewport() -> Viewport {
		Viewport {
			bounds: Bounds::new(Point::default(), gpui::size(px(600.), px(114.))),
			scroll: px(0.),
			scale: 2.,
			shape: Shape::Bar,
		}
	}

	#[test]
	fn leading_edge_arrives_first_and_motion_settles_at_both_refresh_rates() {
		for hz in [60., 165.] {
			let mut motion = Motion::default();
			let start = Instant::now();
			assert!(motion.sample(0, bounds(0., 0.), viewport(), start).is_none());
			motion.sample(40, bounds(400., 0.), viewport(), start);
			let first = motion
				.sample(40, bounds(400., 0.), viewport(), start + Duration::from_secs_f32(1. / hz))
				.unwrap();
			assert!(first[1].x - first[0].x > px(1.5), "the leading edge must lead the trail");
			for frame in 2..100 {
				motion.sample(
					40,
					bounds(400., 0.),
					viewport(),
					start + Duration::from_secs_f32(frame as f32 / hz),
				);
			}
			let state = motion.0.as_ref().unwrap();
			assert!(!state.moving);
			assert_eq!(state.corners.map(|c| c.position), corners(bounds(400., 0.)));
		}
	}

	#[test]
	fn rapid_typing_preserves_position_and_momentum_on_retarget() {
		let mut motion = Motion::default();
		let start = Instant::now();
		motion.sample(0, bounds(0., 0.), viewport(), start);
		motion.sample(1, bounds(6., 0.), viewport(), start);
		let next = start + Duration::from_millis(6);
		motion.sample(1, bounds(6., 0.), viewport(), next);
		let before = motion.0.as_ref().unwrap().corners;
		motion.sample(2, bounds(12., 0.), viewport(), next);
		let after = motion.0.as_ref().unwrap().corners;
		for (before, after) in before.into_iter().zip(after) {
			assert_eq!(before.position, after.position);
			assert_eq!(before.velocity, after.velocity);
		}
	}

	#[test]
	fn scroll_resize_reflow_and_resuming_after_a_pause_snap() {
		let start = Instant::now();
		let mut scrolled = viewport();
		scrolled.scroll = px(19.);
		let mut resized = viewport();
		resized.bounds.size.width = px(480.);
		for (view, logical, elapsed) in [
			(scrolled, 2, Duration::from_millis(10)),
			(resized, 2, Duration::from_millis(10)),
			(viewport(), 1, Duration::from_millis(10)),
			(viewport(), 2, Duration::from_millis(300)),
		] {
			let mut motion = Motion::default();
			motion.sample(0, bounds(0., 0.), viewport(), start);
			motion.sample(1, bounds(120., 0.), viewport(), start);
			assert!(motion.sample(logical, bounds(80., 19.), view, start + elapsed).is_none());
			assert_eq!(
				motion.0.as_ref().unwrap().corners.map(|c| c.position),
				corners(bounds(80., 19.))
			);
		}
	}
}
