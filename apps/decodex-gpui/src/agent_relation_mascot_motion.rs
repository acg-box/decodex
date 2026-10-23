//! Pixel-step poses ported from the approved Cloudlets motion.css.
type Steps = &'static [(f32, f32)];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
	Idle,
	Hopping,
	Working,
	Waiting,
	Done,
	Attention,
}
impl State {
	pub(super) fn for_status(status: &str) -> Self {
		match status {
			"Running" => Self::Working,
			"Waiting" | "Waiting on work" | "Queued" | "Starting" | "Busy" | "Paused" =>
				Self::Waiting,
			"Needs you" | "Approval" | "Input needed" | "Error" | "Failed" => Self::Attention,
			_ => Self::Idle,
		}
	}

	pub(super) fn pose(self, seconds: f32, held_right: bool) -> Pose {
		let mut pose = Pose::default();
		let (period, actor, hands): (f32, Steps, Steps) = match self {
			Self::Hopping => (
				0.9,
				&[(0., 0.), (0.24, -1.), (0.36, -2.), (0.56, -1.), (0.7, 0.)],
				&[(0., 0.), (0.22, -1.), (0.38, -2.), (0.64, -1.), (0.78, 0.)],
			),
			Self::Waiting =>
				(3.6, &[(0., 0.), (0.4, 1.), (0.9, 0.)], &[(0., 0.), (0.48, 1.), (0.96, 0.)]),
			Self::Done => (
				2.2,
				&[
					(0., 0.),
					(0.08, -1.),
					(0.12, -2.),
					(0.16, -3.),
					(0.22, -4.),
					(0.29, -3.),
					(0.34, -2.),
					(0.39, -1.),
					(0.44, 0.),
					(0.51, -1.),
					(0.58, 0.),
				],
				&[(0., 0.), (0.12, -1.), (0.2, -2.), (0.48, -1.), (0.7, 0.)],
			),
			Self::Attention => (2.4, &[(0., 0.), (0.14, -1.), (0.56, 0.)], &[]),
			Self::Working => {
				let cloud_t = ((seconds + if held_right { 0.35 } else { 0. }) / 1.4).fract();
				pose.cloud[1] =
					step(cloud_t, &[(0., 0.), (0.18, -1.), (0.32, 0.), (0.68, -1.), (0.82, 0.)]);
				for (right, hand) in [(false, &mut pose.left), (true, &mut pose.right)] {
					let t = ((seconds + if right { 0.35 } else { 0. }) / 0.7).fract();
					hand[0] =
						if (0.42..0.86).contains(&t) { if right { 1. } else { -1. } } else { 0. };
					hand[1] = step(
						t,
						&[
							(0., 0.),
							(0.24, -1.),
							(0.42, if right == held_right { -1. } else { -2. }),
							(0.64, -1.),
							(0.86, 0.),
						],
					);
				}
				return pose;
			},
			Self::Idle => return pose,
		};
		let t = (seconds / period).fract();
		let y = step(t, actor);
		pose.cloud[1] = y;
		let hand_y = step(t, hands);
		let spread = match self {
			Self::Hopping => (0.38..0.64).contains(&t),
			Self::Done => (0.12..0.62).contains(&t),
			_ => false,
		};
		pose.left = [if spread { -1. } else { 0. }, y + hand_y];
		pose.right = [if spread { 1. } else { 0. }, y + hand_y];
		if self == Self::Attention {
			let hand = if held_right { &mut pose.left } else { &mut pose.right };
			hand[1] += step(
				t,
				&[
					(0., 0.),
					(0.12, -1.),
					(0.18, -2.),
					(0.24, -3.),
					(0.3, -2.),
					(0.36, -3.),
					(0.42, -2.),
					(0.48, -1.),
					(0.58, 0.),
				],
			);
			hand[0] = if (0.18..0.24).contains(&t)
				|| (0.3..0.36).contains(&t)
				|| (0.42..0.48).contains(&t)
			{
				if held_right { -1. } else { 1. }
			} else {
				0.
			};
		}
		pose
	}
}

#[derive(Default)]
pub(super) struct Pose {
	pub cloud: [f32; 2],
	pub left: [f32; 2],
	pub right: [f32; 2],
}
pub(super) struct Particle {
	pub offset: [f32; 2],
	pub size: f32,
	pub opacity: f32,
}
impl Pose {
	pub(super) fn particle(&self, state: State, seconds: f32, index: usize) -> Particle {
		let moving = matches!(state, State::Working | State::Hopping | State::Done);
		if !moving {
			return Particle { offset: [0., 0.], size: 1., opacity: 0.65 };
		}
		let period = if state == State::Done { 2.2 } else { 2.8 };
		let t = (seconds / period + index as f32 / 4.).fract();
		let drift = step(t, &[(0., 0.), (0.25, 1.), (0.4, 2.), (0.55, 3.), (0.7, 4.), (0.85, 5.)]);
		Particle {
			offset: [
				if index == 3 { -6. } else { 0. } + drift,
				if index == 2 {
					-3.
				} else if index == 3 {
					1.
				} else {
					-2.
				} - drift,
			],
			size: if index == 3 || t >= 0.7 { 0.5 } else { 1. },
			opacity: linear(
				t,
				&[(0., 0.), (0.12, 1.), (0.55, 0.85), (0.8, 0.35), (0.94, 0.), (1., 0.)],
			),
		}
	}
}
fn step(t: f32, keys: &[(f32, f32)]) -> f32 {
	keys.iter().rev().find(|(at, _)| t >= *at).map_or(0., |(_, value)| *value)
}
fn linear(t: f32, keys: &[(f32, f32)]) -> f32 {
	let pair = keys.windows(2).find(|pair| t <= pair[1].0).unwrap();
	pair[0].1 + (pair[1].1 - pair[0].1) * ((t - pair[0].0) / (pair[1].0 - pair[0].0))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn attention_keeps_the_held_hand_with_the_body_and_waves_the_free_hand() {
		for held_right in [false, true] {
			let pose = State::Attention.pose(2.4 * 0.25, held_right);
			let (held, free) =
				if held_right { (pose.right, pose.left) } else { (pose.left, pose.right) };
			assert_eq!(held, pose.cloud);
			assert!(free[1] < held[1]);
		}
	}
	#[test]
	fn state_motion_stays_bounded_and_particles_fade() {
		for state in [
			State::Idle,
			State::Hopping,
			State::Working,
			State::Waiting,
			State::Done,
			State::Attention,
		] {
			for frame in 0..280 {
				let seconds = frame as f32 / 50.;
				let pose = state.pose(seconds, true);
				assert!(pose.cloud[1].abs() <= 4.);
				for index in 0..4 {
					let pixel = pose.particle(state, seconds, index);
					assert!((0.0..=1.0).contains(&pixel.opacity));
				}
			}
		}
	}
}
